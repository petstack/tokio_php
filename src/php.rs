use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::ffi::CString;
use std::io::Read;
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::io::FromRawFd;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;

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

static PHP_INITIALIZED: AtomicBool = AtomicBool::new(false);
static PHP_EXECUTOR: OnceCell<PhpExecutor> = OnceCell::new();

#[derive(Debug, Clone)]
pub struct PhpRequest {
    pub script_path: String,
    pub get_params: HashMap<String, String>,
    pub post_params: HashMap<String, String>,
    pub cookies: HashMap<String, String>,
    pub server_vars: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct PhpResponse {
    pub body: String,
    pub headers: Vec<(String, String)>,
}

enum PhpCommand {
    ExecuteRequest(PhpRequest, mpsc::Sender<Result<PhpResponse, String>>),
    Shutdown,
}

struct PhpExecutor {
    sender: mpsc::Sender<PhpCommand>,
}

impl PhpExecutor {
    fn new() -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<PhpCommand>();

        thread::Builder::new()
            .name("php-executor".to_string())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                Self::php_thread(rx);
            })
            .map_err(|e| format!("Failed to spawn PHP thread: {}", e))?;

        Ok(PhpExecutor { sender: tx })
    }

    fn php_thread(rx: mpsc::Receiver<PhpCommand>) {
        unsafe {
            let program_name = CString::new("tokio_php").unwrap();
            let mut argv: [*mut c_char; 2] = [program_name.as_ptr() as *mut c_char, ptr::null_mut()];

            let result = php_embed_init(1, argv.as_mut_ptr());
            if result != 0 {
                tracing::error!("Failed to initialize PHP embed: {}", result);
                return;
            }
        }

        PHP_INITIALIZED.store(true, Ordering::SeqCst);
        tracing::info!("PHP runtime initialized");

        while let Ok(cmd) = rx.recv() {
            match cmd {
                PhpCommand::ExecuteRequest(request, response_tx) => {
                    let result = Self::do_execute_request(&request);
                    let _ = response_tx.send(result);
                }
                PhpCommand::Shutdown => {
                    break;
                }
            }
        }

        unsafe {
            php_embed_shutdown();
        }
        PHP_INITIALIZED.store(false, Ordering::SeqCst);
        tracing::info!("PHP runtime shut down");
    }

    fn escape_php_string(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\0', "")
    }

    fn build_superglobals_code(request: &PhpRequest) -> String {
        let mut code = String::new();

        // Start output buffering at the very beginning
        code.push_str("if (!ob_get_level()) ob_start();\n");

        // Build $_GET
        code.push_str("$_GET = [");
        for (i, (key, value)) in request.get_params.iter().enumerate() {
            if i > 0 {
                code.push_str(", ");
            }
            code.push_str(&format!(
                "'{}' => '{}'",
                Self::escape_php_string(key),
                Self::escape_php_string(value)
            ));
        }
        code.push_str("];\n");

        // Build $_POST
        code.push_str("$_POST = [");
        for (i, (key, value)) in request.post_params.iter().enumerate() {
            if i > 0 {
                code.push_str(", ");
            }
            code.push_str(&format!(
                "'{}' => '{}'",
                Self::escape_php_string(key),
                Self::escape_php_string(value)
            ));
        }
        code.push_str("];\n");

        // Build $_SERVER
        code.push_str("$_SERVER = array_merge($_SERVER ?? [], [");
        for (i, (key, value)) in request.server_vars.iter().enumerate() {
            if i > 0 {
                code.push_str(", ");
            }
            code.push_str(&format!(
                "'{}' => '{}'",
                Self::escape_php_string(key),
                Self::escape_php_string(value)
            ));
        }
        code.push_str("]);\n");

        // Build $_COOKIE
        code.push_str("$_COOKIE = [");
        for (i, (key, value)) in request.cookies.iter().enumerate() {
            if i > 0 {
                code.push_str(", ");
            }
            code.push_str(&format!(
                "'{}' => '{}'",
                Self::escape_php_string(key),
                Self::escape_php_string(value)
            ));
        }
        code.push_str("];\n");

        // Build $_REQUEST (merge of GET and POST, POST takes precedence)
        code.push_str("$_REQUEST = array_merge($_GET, $_POST);\n");

        code
    }

    fn do_execute_request(request: &PhpRequest) -> Result<PhpResponse, String> {
        // First, run superglobals setup WITHOUT stdout redirection
        // This allows ini_set and ob_start to work properly
        unsafe {
            let superglobals_code = Self::build_superglobals_code(request);
            let code_c = CString::new(superglobals_code).map_err(|e| e.to_string())?;
            let name_c = CString::new("superglobals_init").unwrap();

            zend_eval_string(
                code_c.as_ptr() as *mut c_char,
                ptr::null_mut(),
                name_c.as_ptr() as *mut c_char,
            );
        }

        // Now create pipe for stdout capture
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

            // Flush output buffer
            let flush_ob = CString::new("ob_end_flush();").unwrap();
            let name = CString::new("ob_flush").unwrap();
            zend_eval_string(
                flush_ob.as_ptr() as *mut c_char,
                ptr::null_mut(),
                name.as_ptr() as *mut c_char,
            );
        }

        // Capture headers before restoring stdout
        let headers = unsafe { Self::capture_headers() };

        // Close session to ensure data is written
        unsafe {
            let close_session = CString::new(
                "if (session_status() === PHP_SESSION_ACTIVE) { session_write_close(); }"
            ).unwrap();
            let name = CString::new("session_close").unwrap();
            zend_eval_string(
                close_session.as_ptr() as *mut c_char,
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

        // Read output
        let mut output = String::new();
        let mut read_file = unsafe { std::fs::File::from_raw_fd(read_fd) };
        read_file.read_to_string(&mut output).map_err(|e| e.to_string())?;

        if output.is_empty() {
            return Err("PHP script produced no output".to_string());
        }

        Ok(PhpResponse {
            body: output,
            headers,
        })
    }

    unsafe fn capture_headers() -> Vec<(String, String)> {
        // Use a pipe to capture the output of headers_list()
        let mut pipe_fds: [c_int; 2] = [0, 0];
        if libc::pipe(pipe_fds.as_mut_ptr()) != 0 {
            return Vec::new();
        }

        let read_fd = pipe_fds[0];
        let write_fd = pipe_fds[1];

        let original_stdout = libc::dup(1);
        if original_stdout < 0 {
            libc::close(read_fd);
            libc::close(write_fd);
            return Vec::new();
        }

        if libc::dup2(write_fd, 1) < 0 {
            libc::close(read_fd);
            libc::close(write_fd);
            libc::close(original_stdout);
            return Vec::new();
        }

        // Get headers using PHP's headers_list() and print them
        let code = CString::new(
            "foreach (headers_list() as $h) { echo $h . \"\\n\"; }"
        ).unwrap();
        let name = CString::new("get_headers").unwrap();
        zend_eval_string(
            code.as_ptr() as *mut c_char,
            ptr::null_mut(),
            name.as_ptr() as *mut c_char,
        );

        libc::fflush(ptr::null_mut());
        libc::close(write_fd);
        libc::dup2(original_stdout, 1);
        libc::close(original_stdout);

        // Read the headers output
        let mut headers_output = String::new();
        let mut read_file = std::fs::File::from_raw_fd(read_fd);
        let _ = read_file.read_to_string(&mut headers_output);

        // Parse headers
        let mut headers = Vec::new();
        for line in headers_output.lines() {
            if let Some((name, value)) = line.split_once(':') {
                headers.push((name.trim().to_string(), value.trim().to_string()));
            }
        }

        headers
    }

    fn execute_request(&self, request: PhpRequest) -> Result<PhpResponse, String> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(PhpCommand::ExecuteRequest(request, tx))
            .map_err(|e| format!("Failed to send command: {}", e))?;

        rx.recv().map_err(|e| format!("Failed to receive response: {}", e))?
    }

    fn shutdown(&self) {
        let _ = self.sender.send(PhpCommand::Shutdown);
    }
}

pub struct PhpRuntime;

impl PhpRuntime {
    pub fn init() -> Result<(), String> {
        PHP_EXECUTOR
            .get_or_try_init(|| PhpExecutor::new())
            .map(|_| ())
    }

    pub fn shutdown() {
        if let Some(executor) = PHP_EXECUTOR.get() {
            executor.shutdown();
        }
    }

    pub fn execute_request(request: PhpRequest) -> Result<PhpResponse, String> {
        let executor = PHP_EXECUTOR
            .get()
            .ok_or("PHP runtime not initialized")?;

        executor.execute_request(request)
    }

    #[allow(dead_code)]
    pub fn execute_file(file_path: &str) -> Result<PhpResponse, String> {
        Self::execute_request(PhpRequest {
            script_path: file_path.to_string(),
            get_params: HashMap::new(),
            post_params: HashMap::new(),
            cookies: HashMap::new(),
            server_vars: HashMap::new(),
        })
    }
}
