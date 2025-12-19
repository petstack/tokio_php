use nix::sys::signal::{self, Signal};
use nix::sys::wait::waitpid;
use nix::unistd::{fork, ForkResult, Pid};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::CString;
use std::io::{Read, Write};
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::io::FromRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[link(name = "php")]
extern "C" {
    fn php_embed_init(argc: c_int, argv: *mut *mut c_char) -> c_int;
    fn php_embed_shutdown();
    fn zend_eval_string(
        str: *mut c_char,
        retval_ptr: *mut c_void,
        string_name: *mut c_char,
    ) -> c_int;
    fn php_execute_simple_script(handle: *mut ZendFileHandle, retval: *mut c_void) -> c_int;
    fn zend_stream_init_filename(handle: *mut ZendFileHandle, filename: *const c_char);
    fn zend_destroy_file_handle(handle: *mut ZendFileHandle);
}

#[repr(C)]
pub struct ZendFileHandle {
    _data: [u8; 128],
}

static PHP_WORKER_POOL: OnceCell<Arc<PhpWorkerPool>> = OnceCell::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadedFile {
    pub name: String,
    pub mime_type: String,
    pub tmp_name: String,
    pub size: u64,
    pub error: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhpRequest {
    pub script_path: String,
    pub get_params: HashMap<String, String>,
    pub post_params: HashMap<String, String>,
    pub cookies: HashMap<String, String>,
    pub server_vars: HashMap<String, String>,
    pub files: HashMap<String, Vec<UploadedFile>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhpResponse {
    pub body: String,
    pub headers: Vec<(String, String)>,
}

struct PhpWorker {
    pid: Pid,
    socket_path: PathBuf,
}

pub struct PhpWorkerPool {
    workers: Vec<PhpWorker>,
    next_worker: AtomicUsize,
}

impl PhpWorkerPool {
    pub fn new(num_workers: usize) -> Result<Arc<Self>, String> {
        let mut workers = Vec::with_capacity(num_workers);

        for i in 0..num_workers {
            let socket_path = PathBuf::from(format!("/tmp/php_worker_{}.sock", i));

            // Remove old socket if exists
            let _ = std::fs::remove_file(&socket_path);

            match unsafe { fork() } {
                Ok(ForkResult::Parent { child }) => {
                    // Parent process - wait for socket to be ready
                    for _ in 0..100 {
                        if socket_path.exists() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }

                    workers.push(PhpWorker {
                        pid: child,
                        socket_path,
                    });
                    tracing::info!("Spawned PHP worker {} (PID: {})", i, child);
                }
                Ok(ForkResult::Child) => {
                    // Child process - become PHP worker
                    Self::run_worker(i, &socket_path);
                    std::process::exit(0);
                }
                Err(e) => {
                    return Err(format!("Fork failed: {}", e));
                }
            }
        }

        Ok(Arc::new(PhpWorkerPool {
            workers,
            next_worker: AtomicUsize::new(0),
        }))
    }

    fn run_worker(id: usize, socket_path: &PathBuf) {
        // Initialize PHP in this process
        unsafe {
            let program_name = CString::new("tokio_php_worker").unwrap();
            let mut argv: [*mut c_char; 2] = [program_name.as_ptr() as *mut c_char, ptr::null_mut()];

            let result = php_embed_init(1, argv.as_mut_ptr());
            if result != 0 {
                eprintln!("Worker {}: Failed to initialize PHP embed: {}", id, result);
                return;
            }
        }

        // Create Unix socket listener
        let listener = match UnixListener::bind(socket_path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Worker {}: Failed to bind socket: {}", id, e);
                return;
            }
        };

        eprintln!("Worker {}: PHP initialized, listening on {:?}", id, socket_path);

        // Accept and handle connections (one request per connection)
        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    let _ = Self::handle_request(&mut stream);
                }
                Err(e) => {
                    eprintln!("Worker {}: Accept error: {}", id, e);
                }
            }
        }

        unsafe {
            php_embed_shutdown();
        }
    }

    fn handle_request(stream: &mut UnixStream) -> Result<(), String> {
        // Read request length (4 bytes, big-endian)
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).map_err(|e| e.to_string())?;
        let len = u32::from_be_bytes(len_buf) as usize;

        // Read request (bincode)
        let mut request_buf = vec![0u8; len];
        stream.read_exact(&mut request_buf).map_err(|e| e.to_string())?;

        let request: PhpRequest = bincode::deserialize(&request_buf)
            .map_err(|e| e.to_string())?;

        // Execute PHP
        let response = Self::execute_php(&request)?;

        // Send response (bincode)
        let response_bytes = bincode::serialize(&response).map_err(|e| e.to_string())?;
        let len_bytes = (response_bytes.len() as u32).to_be_bytes();
        stream.write_all(&len_bytes).map_err(|e| e.to_string())?;
        stream.write_all(&response_bytes).map_err(|e| e.to_string())?;

        Ok(())
    }

    const HEADER_DELIMITER: &'static str = "\n---@TOKIO_PHP_HDR@---\n";

    fn execute_php(request: &PhpRequest) -> Result<PhpResponse, String> {
        // Build superglobals code
        let superglobals_code = Self::build_superglobals_code(request);

        unsafe {
            let code_c = CString::new(superglobals_code).map_err(|e| e.to_string())?;
            let name_c = CString::new("superglobals_init").unwrap();

            zend_eval_string(
                code_c.as_ptr() as *mut c_char,
                ptr::null_mut(),
                name_c.as_ptr() as *mut c_char,
            );
        }

        // Create pipe for stdout capture
        let mut pipe_fds: [c_int; 2] = [0, 0];
        if unsafe { libc::pipe(pipe_fds.as_mut_ptr()) } != 0 {
            return Err("Failed to create pipe".to_string());
        }

        let read_fd = pipe_fds[0];
        let write_fd = pipe_fds[1];

        let original_stdout = unsafe { libc::dup(1) };
        if original_stdout < 0 {
            unsafe {
                libc::close(read_fd);
                libc::close(write_fd);
            }
            return Err("Failed to dup stdout".to_string());
        }

        if unsafe { libc::dup2(write_fd, 1) } < 0 {
            unsafe {
                libc::close(read_fd);
                libc::close(write_fd);
                libc::close(original_stdout);
            }
            return Err("Failed to redirect stdout".to_string());
        }

        // Execute the script
        unsafe {
            let path_c = CString::new(request.script_path.as_str()).map_err(|e| e.to_string())?;

            let mut file_handle: ZendFileHandle = std::mem::zeroed();
            zend_stream_init_filename(&mut file_handle, path_c.as_ptr());

            php_execute_simple_script(&mut file_handle, ptr::null_mut());

            zend_destroy_file_handle(&mut file_handle);

            // Flush output buffer and capture headers
            let capture_code = CString::new(format!(
                "ob_end_flush();\n\
                 if (session_status() === PHP_SESSION_ACTIVE) session_write_close();\n\
                 echo \"{}\";\n\
                 $__c = http_response_code();\n\
                 if ($__c && $__c !== 200) echo \"Status: $__c\\n\";\n\
                 foreach (headers_list() as $h) echo $h . \"\\n\";",
                Self::HEADER_DELIMITER
            )).unwrap();
            let name = CString::new("finalize").unwrap();
            zend_eval_string(
                capture_code.as_ptr() as *mut c_char,
                ptr::null_mut(),
                name.as_ptr() as *mut c_char,
            );
        }

        // Flush and restore stdout
        unsafe {
            libc::fflush(ptr::null_mut());
            libc::close(write_fd);
            libc::dup2(original_stdout, 1);
            libc::close(original_stdout);
        }

        // Read combined output
        let mut combined = String::new();
        let mut read_file = unsafe { std::fs::File::from_raw_fd(read_fd) };
        read_file.read_to_string(&mut combined).map_err(|e| e.to_string())?;

        // Split body and headers
        let (body, headers) = if let Some(pos) = combined.find(Self::HEADER_DELIMITER) {
            let body = combined[..pos].to_string();
            let headers_str = &combined[pos + Self::HEADER_DELIMITER.len()..];
            let headers = Self::parse_headers_str(headers_str);
            (body, headers)
        } else {
            (combined, Vec::new())
        };

        Ok(PhpResponse { body, headers })
    }

    fn escape_php_string(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\0', "")
    }

    fn build_superglobals_code(request: &PhpRequest) -> String {
        let mut code = String::with_capacity(4096);

        code.push_str("header_remove();\n");
        code.push_str("http_response_code(200);\n");
        code.push_str("if (!ob_get_level()) ob_start();\n");

        // $_GET
        code.push_str("$_GET = [");
        for (i, (key, value)) in request.get_params.iter().enumerate() {
            if i > 0 { code.push_str(", "); }
            code.push_str(&format!("'{}' => '{}'",
                Self::escape_php_string(key),
                Self::escape_php_string(value)));
        }
        code.push_str("];\n");

        // $_POST
        code.push_str("$_POST = [");
        for (i, (key, value)) in request.post_params.iter().enumerate() {
            if i > 0 { code.push_str(", "); }
            code.push_str(&format!("'{}' => '{}'",
                Self::escape_php_string(key),
                Self::escape_php_string(value)));
        }
        code.push_str("];\n");

        // $_SERVER
        code.push_str("$_SERVER = array_merge($_SERVER ?? [], [");
        for (i, (key, value)) in request.server_vars.iter().enumerate() {
            if i > 0 { code.push_str(", "); }
            code.push_str(&format!("'{}' => '{}'",
                Self::escape_php_string(key),
                Self::escape_php_string(value)));
        }
        code.push_str("]);\n");

        // $_COOKIE
        code.push_str("$_COOKIE = [");
        for (i, (key, value)) in request.cookies.iter().enumerate() {
            if i > 0 { code.push_str(", "); }
            code.push_str(&format!("'{}' => '{}'",
                Self::escape_php_string(key),
                Self::escape_php_string(value)));
        }
        code.push_str("];\n");

        code.push_str("$_REQUEST = array_merge($_GET, $_POST);\n");

        // $_FILES
        code.push_str("$_FILES = [\n");
        for (i, (field_name, files_vec)) in request.files.iter().enumerate() {
            if i > 0 { code.push_str(",\n"); }

            if files_vec.len() == 1 {
                let file = &files_vec[0];
                code.push_str(&format!(
                    "  '{}' => ['name' => '{}', 'type' => '{}', 'tmp_name' => '{}', 'error' => {}, 'size' => {}]",
                    Self::escape_php_string(field_name),
                    Self::escape_php_string(&file.name),
                    Self::escape_php_string(&file.mime_type),
                    Self::escape_php_string(&file.tmp_name),
                    file.error,
                    file.size
                ));
            } else {
                let names: Vec<String> = files_vec.iter()
                    .map(|f| format!("'{}'", Self::escape_php_string(&f.name)))
                    .collect();
                let types: Vec<String> = files_vec.iter()
                    .map(|f| format!("'{}'", Self::escape_php_string(&f.mime_type)))
                    .collect();
                let tmp_names: Vec<String> = files_vec.iter()
                    .map(|f| format!("'{}'", Self::escape_php_string(&f.tmp_name)))
                    .collect();
                let errors: Vec<String> = files_vec.iter()
                    .map(|f| f.error.to_string())
                    .collect();
                let sizes: Vec<String> = files_vec.iter()
                    .map(|f| f.size.to_string())
                    .collect();

                code.push_str(&format!(
                    "  '{}' => ['name' => [{}], 'type' => [{}], 'tmp_name' => [{}], 'error' => [{}], 'size' => [{}]]",
                    Self::escape_php_string(field_name),
                    names.join(", "), types.join(", "), tmp_names.join(", "),
                    errors.join(", "), sizes.join(", ")
                ));
            }
        }
        code.push_str("\n];\n");

        code
    }

    fn parse_headers_str(headers_str: &str) -> Vec<(String, String)> {
        headers_str
            .lines()
            .filter_map(|line| {
                line.split_once(':').map(|(name, value)| {
                    (name.trim().to_string(), value.trim().to_string())
                })
            })
            .collect()
    }

    pub async fn execute_request(&self, request: PhpRequest) -> Result<PhpResponse, String> {
        // Round-robin worker selection
        let worker_idx = self.next_worker.fetch_add(1, Ordering::Relaxed) % self.workers.len();
        let socket_path = self.workers[worker_idx].socket_path.clone();

        // Execute in blocking task
        tokio::task::spawn_blocking(move || {
            Self::send_request(&socket_path, &request)
        })
        .await
        .map_err(|e| format!("Task join error: {}", e))?
    }

    fn send_request(socket_path: &PathBuf, request: &PhpRequest) -> Result<PhpResponse, String> {
        let mut stream = UnixStream::connect(socket_path)
            .map_err(|e| format!("Failed to connect: {}", e))?;

        // Send request (bincode)
        let request_bytes = bincode::serialize(request).map_err(|e| e.to_string())?;
        let len_bytes = (request_bytes.len() as u32).to_be_bytes();
        stream.write_all(&len_bytes).map_err(|e| e.to_string())?;
        stream.write_all(&request_bytes).map_err(|e| e.to_string())?;

        // Read response length
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).map_err(|e| e.to_string())?;
        let len = u32::from_be_bytes(len_buf) as usize;

        // Read response
        let mut response_buf = vec![0u8; len];
        stream.read_exact(&mut response_buf).map_err(|e| e.to_string())?;

        bincode::deserialize(&response_buf).map_err(|e| e.to_string())
    }

    pub fn shutdown(&self) {
        for worker in &self.workers {
            let _ = signal::kill(worker.pid, Signal::SIGTERM);
            let _ = waitpid(worker.pid, None);
            let _ = std::fs::remove_file(&worker.socket_path);
        }
    }
}

impl Drop for PhpWorkerPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub struct PhpRuntime;

impl PhpRuntime {
    pub fn init() -> Result<(), String> {
        Self::init_with_workers(num_cpus::get())
    }

    pub fn init_with_workers(num_workers: usize) -> Result<(), String> {
        PHP_WORKER_POOL
            .get_or_try_init(|| PhpWorkerPool::new(num_workers))
            .map(|_| ())
    }

    pub fn shutdown() {
        if let Some(pool) = PHP_WORKER_POOL.get() {
            pool.shutdown();
        }
    }

    pub async fn execute_request(request: PhpRequest) -> Result<PhpResponse, String> {
        let pool = PHP_WORKER_POOL
            .get()
            .ok_or("PHP runtime not initialized")?;

        pool.execute_request(request).await
    }

    pub fn worker_count() -> usize {
        PHP_WORKER_POOL
            .get()
            .map(|p| p.workers.len())
            .unwrap_or(0)
    }
}
