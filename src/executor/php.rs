use async_trait::async_trait;
use std::ffi::CString;
use std::io::Read;
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::io::FromRawFd;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use tokio::sync::oneshot;

use super::{ExecutorError, ScriptExecutor};
use crate::types::{ScriptRequest, ScriptResponse};

// PHP embed FFI bindings
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

    // ZTS-specific: Thread resource management
    fn ts_resource_ex(id: c_int, th_id: *mut c_void) -> *mut c_void;

    // Request lifecycle (needed for ZTS multi-threading)
    fn php_request_startup() -> c_int;
    fn php_request_shutdown(dummy: *mut c_void);
}

#[repr(C)]
struct ZendFileHandle {
    _data: [u8; 128],
}

/// Request sent to PHP worker thread.
struct WorkerRequest {
    request: ScriptRequest,
    response_tx: oneshot::Sender<Result<ScriptResponse, String>>,
}

/// PHP worker thread handle.
struct PhpWorkerThread {
    handle: JoinHandle<()>,
}

/// Thread-based PHP worker pool for ZTS PHP.
struct PhpThreadPool {
    request_tx: mpsc::Sender<WorkerRequest>,
    workers: Vec<PhpWorkerThread>,
    worker_count: AtomicUsize,
}

impl PhpThreadPool {
    const HEADER_DELIMITER: &'static str = "\n---@TOKIO_PHP_HDR@---\n";

    fn new(num_workers: usize) -> Result<Self, String> {
        // Initialize PHP once in main thread (ZTS mode)
        unsafe {
            let program_name = CString::new("tokio_php").unwrap();
            let mut argv: [*mut c_char; 2] = [program_name.as_ptr() as *mut c_char, ptr::null_mut()];

            let result = php_embed_init(1, argv.as_mut_ptr());
            if result != 0 {
                return Err(format!("Failed to initialize PHP embed: {}", result));
            }
        }
        tracing::info!("PHP ZTS initialized in main thread");

        let (request_tx, request_rx) = mpsc::channel::<WorkerRequest>();
        let request_rx = Arc::new(Mutex::new(request_rx));

        let mut workers = Vec::with_capacity(num_workers);

        for id in 0..num_workers {
            let rx = Arc::clone(&request_rx);

            let handle = thread::Builder::new()
                .name(format!("php-worker-{}", id))
                .spawn(move || {
                    Self::worker_thread_main(id, rx);
                })
                .map_err(|e| format!("Failed to spawn worker thread {}: {}", id, e))?;

            workers.push(PhpWorkerThread { handle });
            tracing::info!("Spawned PHP worker thread {}", id);
        }

        Ok(Self {
            request_tx,
            workers,
            worker_count: AtomicUsize::new(num_workers),
        })
    }

    fn worker_thread_main(id: usize, rx: Arc<Mutex<mpsc::Receiver<WorkerRequest>>>) {
        // For ZTS: allocate thread-local storage for this thread
        unsafe {
            let _ = ts_resource_ex(0, ptr::null_mut());
        }

        tracing::debug!("Worker {}: Thread-local storage initialized", id);

        // Process requests
        loop {
            let work = {
                let guard = rx.lock().unwrap();
                guard.recv()
            };

            match work {
                Ok(WorkerRequest { request, response_tx }) => {
                    // Start PHP request context for this thread
                    let startup_ok = unsafe { php_request_startup() } == 0;

                    let result = if startup_ok {
                        let res = Self::execute_php(&request);
                        // End PHP request context
                        unsafe { php_request_shutdown(ptr::null_mut()); }
                        res
                    } else {
                        Err("Failed to start PHP request".to_string())
                    };

                    let _ = response_tx.send(result);
                }
                Err(_) => {
                    // Channel closed, shutdown
                    break;
                }
            }
        }

        tracing::debug!("Worker {}: Shutdown complete", id);
    }

    fn execute_php(request: &ScriptRequest) -> Result<ScriptResponse, String> {
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

        // Use temp file instead of pipe to avoid buffer deadlock with large output
        let tmp_path = format!("/tmp/php_out_{:?}.tmp", std::thread::current().id());
        let tmp_path_c = CString::new(tmp_path.as_str()).unwrap();

        let write_fd = unsafe {
            libc::open(
                tmp_path_c.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
                0o600,
            )
        };
        if write_fd < 0 {
            return Err("Failed to create temp file".to_string());
        }

        let original_stdout = unsafe { libc::dup(1) };
        if original_stdout < 0 {
            unsafe { libc::close(write_fd); }
            let _ = std::fs::remove_file(&tmp_path);
            return Err("Failed to dup stdout".to_string());
        }

        if unsafe { libc::dup2(write_fd, 1) } < 0 {
            unsafe {
                libc::close(write_fd);
                libc::close(original_stdout);
            }
            let _ = std::fs::remove_file(&tmp_path);
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
                "while(ob_get_level())ob_end_flush();\n\
                 echo\"{}\";\n\
                 if(($__c=http_response_code())&&$__c!=200)echo\"Status:$__c\\n\";\n\
                 foreach(headers_list()as$h)echo$h.\"\\n\";",
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

        // Read combined output from temp file
        let combined = std::fs::read_to_string(&tmp_path).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&tmp_path);

        // Split body and headers
        let (body, headers) = if let Some(pos) = combined.find(Self::HEADER_DELIMITER) {
            let body = combined[..pos].to_string();
            let headers_str = &combined[pos + Self::HEADER_DELIMITER.len()..];
            let headers = Self::parse_headers_str(headers_str);
            (body, headers)
        } else {
            (combined, Vec::new())
        };

        Ok(ScriptResponse { body, headers })
    }

    fn escape_php_string(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\0', "")
    }

    fn build_superglobals_code(request: &ScriptRequest) -> String {
        let mut code = String::with_capacity(4096);

        code.push_str("header_remove();\n");
        code.push_str("http_response_code(200);\n");
        code.push_str("if (!ob_get_level()) ob_start();\n");

        // $_GET
        code.push_str("$_GET = [");
        for (i, (key, value)) in request.get_params.iter().enumerate() {
            if i > 0 { code.push(','); }
            code.push_str(&format!("'{}'=>'{}'",
                Self::escape_php_string(key),
                Self::escape_php_string(value)));
        }
        code.push_str("];\n");

        // $_POST
        code.push_str("$_POST = [");
        for (i, (key, value)) in request.post_params.iter().enumerate() {
            if i > 0 { code.push(','); }
            code.push_str(&format!("'{}'=>'{}'",
                Self::escape_php_string(key),
                Self::escape_php_string(value)));
        }
        code.push_str("];\n");

        // $_SERVER
        code.push_str("$_SERVER = array_merge($_SERVER ?? [], [");
        for (i, (key, value)) in request.server_vars.iter().enumerate() {
            if i > 0 { code.push(','); }
            code.push_str(&format!("'{}'=>'{}'",
                Self::escape_php_string(key),
                Self::escape_php_string(value)));
        }
        code.push_str("]);\n");

        // $_COOKIE
        code.push_str("$_COOKIE = [");
        for (i, (key, value)) in request.cookies.iter().enumerate() {
            if i > 0 { code.push(','); }
            code.push_str(&format!("'{}'=>'{}'",
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
                    "  '{}' => ['name'=>'{}','type'=>'{}','tmp_name'=>'{}','error'=>{},'size'=>{}]",
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
                    "  '{}' => ['name'=>[{}],'type'=>[{}],'tmp_name'=>[{}],'error'=>[{}],'size'=>[{}]]",
                    Self::escape_php_string(field_name),
                    names.join(","), types.join(","), tmp_names.join(","),
                    errors.join(","), sizes.join(",")
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

    async fn execute_request(&self, request: ScriptRequest) -> Result<ScriptResponse, String> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(WorkerRequest { request, response_tx })
            .map_err(|_| "Worker pool shut down".to_string())?;

        response_rx
            .await
            .map_err(|_| "Worker dropped response".to_string())?
    }

    fn shutdown(&self) {
        // Drop sender will close the channel, workers will exit
    }

    fn worker_count(&self) -> usize {
        self.worker_count.load(Ordering::Relaxed)
    }
}

impl Drop for PhpThreadPool {
    fn drop(&mut self) {
        // Workers will exit when channel is closed (request_tx dropped)
        // Wait for all workers to finish
        for worker in self.workers.drain(..) {
            let _ = worker.handle.join();
        }

        // Shutdown PHP (it was initialized in main thread)
        unsafe {
            php_embed_shutdown();
        }
        tracing::info!("PHP shutdown complete");
    }
}

/// PHP script executor using ZTS php-embed SAPI with thread pool.
pub struct PhpExecutor {
    pool: PhpThreadPool,
}

impl PhpExecutor {
    /// Creates a new PHP executor with the specified number of worker threads.
    pub fn new(num_workers: usize) -> Result<Self, ExecutorError> {
        let pool = PhpThreadPool::new(num_workers)?;
        Ok(Self { pool })
    }

    /// Returns the number of worker threads.
    pub fn worker_count(&self) -> usize {
        self.pool.worker_count()
    }
}

#[async_trait]
impl ScriptExecutor for PhpExecutor {
    async fn execute(&self, request: ScriptRequest) -> Result<ScriptResponse, ExecutorError> {
        self.pool
            .execute_request(request)
            .await
            .map_err(ExecutorError::from)
    }

    fn name(&self) -> &'static str {
        "php-zts"
    }

    fn shutdown(&self) {
        self.pool.shutdown();
    }
}
