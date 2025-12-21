//! Alternative PHP executor using SAPI module initialization.
//!
//! This executor provides the same functionality as PhpExecutor but uses
//! a separate SAPI initialization path. It was originally intended to use
//! SAPI callbacks for direct output capture, but PHP 8.4's output layer
//! caches callbacks at startup, requiring stdout redirection instead.
//!
//! Both executors perform similarly in benchmarks.

use async_trait::async_trait;
use std::ffi::{c_char, c_int, c_void, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;
use tokio::sync::oneshot;

use super::sapi;
use super::{ExecutorError, ScriptExecutor};
use crate::profiler::{self, ProfileData};
use crate::types::{ScriptRequest, ScriptResponse};

// =============================================================================
// PHP FFI Bindings
// =============================================================================

#[repr(C)]
struct ZendFileHandle {
    _data: [u8; 128],
}

#[link(name = "php")]
extern "C" {
    fn php_request_startup() -> c_int;
    fn php_request_shutdown(dummy: *mut c_void);
    fn php_execute_script(file_handle: *mut ZendFileHandle) -> c_int;
    fn zend_stream_init_filename(handle: *mut ZendFileHandle, filename: *const c_char);
    fn zend_destroy_file_handle(handle: *mut ZendFileHandle);
    fn ts_resource_ex(id: c_int, th_id: *mut c_void) -> *mut c_void;
    fn zend_eval_string(str: *mut c_char, retval: *mut c_void, name: *mut c_char) -> c_int;
}

// Pre-built finalize code - flush output buffers and output headers (same as php.rs)
static FINALIZE_CODE: &[u8] = b"while(ob_get_level())ob_end_flush();echo\"\\n---@TOKIO_PHP_HDR@---\\n\";if(($__c=http_response_code())&&$__c!=200)echo\"Status:$__c\\n\";foreach(headers_list()as$h)echo$h.\"\\n\";\0";
static FINALIZE_NAME: &[u8] = b"f\0";

// CString for memfd
static MEMFD_NAME: &[u8] = b"php_out\0";

// Header delimiter
const HEADER_DELIMITER: &str = "\n---@TOKIO_PHP_HDR@---\n";

// =============================================================================
// Worker Pool
// =============================================================================

struct WorkerRequest {
    request: ScriptRequest,
    response_tx: oneshot::Sender<Result<ScriptResponse, String>>,
    queued_at: Instant,
}

struct PhpWorkerThread {
    handle: JoinHandle<()>,
}

struct PhpSapiPool {
    request_tx: mpsc::Sender<WorkerRequest>,
    workers: Vec<PhpWorkerThread>,
    worker_count: AtomicUsize,
}

impl PhpSapiPool {
    fn new(num_workers: usize) -> Result<Self, String> {
        // Initialize custom SAPI
        sapi::init()?;

        let (request_tx, request_rx) = mpsc::channel::<WorkerRequest>();
        let request_rx = Arc::new(Mutex::new(request_rx));

        let mut workers = Vec::with_capacity(num_workers);

        for id in 0..num_workers {
            let rx = Arc::clone(&request_rx);

            let handle = thread::Builder::new()
                .name(format!("php-sapi-{}", id))
                .spawn(move || {
                    Self::worker_thread_main(id, rx);
                })
                .map_err(|e| format!("Failed to spawn worker thread {}: {}", id, e))?;

            workers.push(PhpWorkerThread { handle });
            tracing::debug!("Spawned PHP SAPI worker thread {}", id);
        }

        tracing::info!("PHP SAPI pool initialized with {} workers", num_workers);

        Ok(Self {
            request_tx,
            workers,
            worker_count: AtomicUsize::new(num_workers),
        })
    }

    fn worker_thread_main(id: usize, rx: Arc<Mutex<mpsc::Receiver<WorkerRequest>>>) {
        // Initialize thread-local storage for ZTS
        unsafe {
            let _ = ts_resource_ex(0, ptr::null_mut());
        }

        tracing::debug!("Worker {}: Thread-local storage initialized", id);

        loop {
            let work = {
                let guard = rx.lock().unwrap();
                guard.recv()
            };

            match work {
                Ok(WorkerRequest { request, response_tx, queued_at }) => {
                    let profiling = request.profile && profiler::is_enabled();

                    let queue_wait_us = if profiling {
                        queued_at.elapsed().as_micros() as u64
                    } else {
                        0
                    };

                    // Note: Not using SAPI callbacks for output/headers anymore

                    // Start PHP request
                    let startup_start = Instant::now();
                    let startup_ok = unsafe { php_request_startup() } == 0;
                    let php_startup_us = if profiling {
                        startup_start.elapsed().as_micros() as u64
                    } else {
                        0
                    };

                    let result = if startup_ok {
                        let res = Self::execute_php(&request, profiling, queue_wait_us, php_startup_us);

                        let shutdown_start = Instant::now();
                        unsafe { php_request_shutdown(ptr::null_mut()); }

                        if profiling {
                            if let Ok(mut resp) = res {
                                if let Some(ref mut profile) = resp.profile {
                                    profile.php_shutdown_us = shutdown_start.elapsed().as_micros() as u64;
                                    profile.total_us = profile.queue_wait_us
                                        + profile.php_startup_us
                                        + profile.superglobals_us
                                        + profile.script_exec_us
                                        + profile.output_capture_us
                                        + profile.php_shutdown_us;
                                }
                                Ok(resp)
                            } else {
                                res
                            }
                        } else {
                            res
                        }
                    } else {
                        Err("Failed to start PHP request".to_string())
                    };

                    let _ = response_tx.send(result);
                }
                Err(_) => {
                    break;
                }
            }
        }

        tracing::debug!("Worker {}: Shutdown complete", id);
    }

    fn execute_php(
        request: &ScriptRequest,
        profiling: bool,
        queue_wait_us: u64,
        php_startup_us: u64,
    ) -> Result<ScriptResponse, String> {
        // Build and execute superglobals code
        let superglobals_start = Instant::now();
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
        let superglobals_us = if profiling {
            superglobals_start.elapsed().as_micros() as u64
        } else {
            0
        };

        // Set up memfd for stdout capture
        let write_fd = unsafe {
            libc::syscall(
                libc::SYS_memfd_create,
                MEMFD_NAME.as_ptr(),
                0 as libc::c_uint,
            ) as libc::c_int
        };
        if write_fd < 0 {
            return Err("Failed to create memfd".to_string());
        }

        let original_stdout = unsafe { libc::dup(1) };
        if original_stdout < 0 {
            unsafe { libc::close(write_fd); }
            return Err("Failed to dup stdout".to_string());
        }

        if unsafe { libc::dup2(write_fd, 1) } < 0 {
            unsafe {
                libc::close(write_fd);
                libc::close(original_stdout);
            }
            return Err("Failed to redirect stdout".to_string());
        }

        // Execute the script
        let script_start = Instant::now();
        unsafe {
            let path_c = CString::new(request.script_path.as_str()).map_err(|e| e.to_string())?;

            let mut file_handle: ZendFileHandle = std::mem::zeroed();
            zend_stream_init_filename(&mut file_handle, path_c.as_ptr());

            php_execute_script(&mut file_handle);

            zend_destroy_file_handle(&mut file_handle);
        }
        let script_exec_us = if profiling {
            script_start.elapsed().as_micros() as u64
        } else {
            0
        };

        // Flush output buffers and restore stdout
        let output_start = Instant::now();
        unsafe {
            zend_eval_string(
                FINALIZE_CODE.as_ptr() as *mut c_char,
                ptr::null_mut(),
                FINALIZE_NAME.as_ptr() as *mut c_char,
            );

            libc::fflush(ptr::null_mut());
            libc::dup2(original_stdout, 1);
            libc::close(original_stdout);
        }

        // Read output from memfd and parse headers
        let (body, headers) = unsafe {
            libc::lseek(write_fd, 0, libc::SEEK_SET);

            let mut output = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let n = libc::read(write_fd, chunk.as_mut_ptr() as *mut libc::c_void, chunk.len());
                if n <= 0 {
                    break;
                }
                output.extend_from_slice(&chunk[..n as usize]);
            }

            libc::close(write_fd);
            let combined = String::from_utf8_lossy(&output);

            // Split body and headers
            if let Some(pos) = combined.find(HEADER_DELIMITER) {
                let body = combined[..pos].to_string();
                let headers_str = &combined[pos + HEADER_DELIMITER.len()..];
                let headers = Self::parse_headers_str(headers_str);
                (body, headers)
            } else {
                (combined.into_owned(), Vec::new())
            }
        };

        let output_capture_us = if profiling {
            output_start.elapsed().as_micros() as u64
        } else {
            0
        };

        let profile = if profiling {
            Some(ProfileData {
                total_us: 0,
                parse_request_us: 0,
                queue_wait_us,
                php_startup_us,
                superglobals_us,
                script_exec_us,
                output_capture_us,
                php_shutdown_us: 0,
                response_build_us: 0,
            })
        } else {
            None
        };

        Ok(ScriptResponse { body, headers, profile })
    }

    async fn execute_request(&self, request: ScriptRequest) -> Result<ScriptResponse, String> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(WorkerRequest {
                request,
                response_tx,
                queued_at: Instant::now(),
            })
            .map_err(|_| "Worker pool shut down".to_string())?;

        response_rx
            .await
            .map_err(|_| "Worker dropped response".to_string())?
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

    /// Check if string needs PHP escaping
    #[inline]
    fn needs_escape(s: &str) -> bool {
        s.bytes().any(|b| b == b'\\' || b == b'\'' || b == 0)
    }

    /// Write PHP-escaped string directly to buffer (zero-alloc for clean strings)
    #[inline]
    fn write_escaped(buf: &mut String, s: &str) {
        if !Self::needs_escape(s) {
            buf.push_str(s);
            return;
        }
        for c in s.chars() {
            match c {
                '\\' => buf.push_str("\\\\"),
                '\'' => buf.push_str("\\'"),
                '\0' => {} // skip null bytes
                _ => buf.push(c),
            }
        }
    }

    /// Write a key-value pair: 'key'=>'value'
    #[inline]
    fn write_kv(buf: &mut String, key: &str, value: &str) {
        buf.push('\'');
        Self::write_escaped(buf, key);
        buf.push_str("'=>'");
        Self::write_escaped(buf, value);
        buf.push('\'');
    }

    fn build_superglobals_code(request: &ScriptRequest) -> String {
        // Estimate capacity: base + params
        let estimated = 256
            + request.get_params.len() * 64
            + request.post_params.len() * 64
            + request.server_vars.len() * 80
            + request.cookies.len() * 64
            + request.files.len() * 200;
        let mut code = String::with_capacity(estimated);

        code.push_str("header_remove();http_response_code(200);if(!ob_get_level())ob_start();");

        // $_GET
        code.push_str("$_GET=[");
        for (i, (key, value)) in request.get_params.iter().enumerate() {
            if i > 0 { code.push(','); }
            Self::write_kv(&mut code, key, value);
        }
        code.push_str("];");

        // $_POST
        code.push_str("$_POST=[");
        for (i, (key, value)) in request.post_params.iter().enumerate() {
            if i > 0 { code.push(','); }
            Self::write_kv(&mut code, key, value);
        }
        code.push_str("];");

        // $_SERVER - direct assignment is faster than array_merge
        code.push_str("$_SERVER=[");
        for (i, (key, value)) in request.server_vars.iter().enumerate() {
            if i > 0 { code.push(','); }
            Self::write_kv(&mut code, key, value);
        }
        code.push_str("];");

        // $_COOKIE
        code.push_str("$_COOKIE=[");
        for (i, (key, value)) in request.cookies.iter().enumerate() {
            if i > 0 { code.push(','); }
            Self::write_kv(&mut code, key, value);
        }
        code.push_str("];");

        code.push_str("$_REQUEST=$_GET+$_POST;");

        // $_FILES - only if there are files
        if request.files.is_empty() {
            code.push_str("$_FILES=[];");
        } else {
            code.push_str("$_FILES=[");
            for (i, (field_name, files_vec)) in request.files.iter().enumerate() {
                if i > 0 { code.push(','); }
                code.push('\'');
                Self::write_escaped(&mut code, field_name);
                code.push_str("'=>");

                if files_vec.len() == 1 {
                    let f = &files_vec[0];
                    code.push_str("['name'=>'");
                    Self::write_escaped(&mut code, &f.name);
                    code.push_str("','type'=>'");
                    Self::write_escaped(&mut code, &f.mime_type);
                    code.push_str("','tmp_name'=>'");
                    Self::write_escaped(&mut code, &f.tmp_name);
                    code.push_str("','error'=>");
                    code.push_str(&f.error.to_string());
                    code.push_str(",'size'=>");
                    code.push_str(&f.size.to_string());
                    code.push(']');
                } else {
                    code.push_str("['name'=>[");
                    for (j, f) in files_vec.iter().enumerate() {
                        if j > 0 { code.push(','); }
                        code.push('\'');
                        Self::write_escaped(&mut code, &f.name);
                        code.push('\'');
                    }
                    code.push_str("],'type'=>[");
                    for (j, f) in files_vec.iter().enumerate() {
                        if j > 0 { code.push(','); }
                        code.push('\'');
                        Self::write_escaped(&mut code, &f.mime_type);
                        code.push('\'');
                    }
                    code.push_str("],'tmp_name'=>[");
                    for (j, f) in files_vec.iter().enumerate() {
                        if j > 0 { code.push(','); }
                        code.push('\'');
                        Self::write_escaped(&mut code, &f.tmp_name);
                        code.push('\'');
                    }
                    code.push_str("],'error'=>[");
                    for (j, f) in files_vec.iter().enumerate() {
                        if j > 0 { code.push(','); }
                        code.push_str(&f.error.to_string());
                    }
                    code.push_str("],'size'=>[");
                    for (j, f) in files_vec.iter().enumerate() {
                        if j > 0 { code.push(','); }
                        code.push_str(&f.size.to_string());
                    }
                    code.push_str("]]");
                }
            }
            code.push_str("];");
        }

        code
    }

    fn shutdown(&self) {
        // Channel close will signal workers to exit
    }

    fn worker_count(&self) -> usize {
        self.worker_count.load(Ordering::Relaxed)
    }
}

impl Drop for PhpSapiPool {
    fn drop(&mut self) {
        for worker in self.workers.drain(..) {
            let _ = worker.handle.join();
        }

        sapi::shutdown();
    }
}

// =============================================================================
// Public Executor Interface
// =============================================================================

/// PHP script executor using custom SAPI for direct output/header capture.
pub struct PhpSapiExecutor {
    pool: PhpSapiPool,
}

impl PhpSapiExecutor {
    /// Creates a new PHP SAPI executor with the specified number of worker threads.
    pub fn new(num_workers: usize) -> Result<Self, ExecutorError> {
        let pool = PhpSapiPool::new(num_workers)?;
        Ok(Self { pool })
    }

    /// Returns the number of worker threads.
    pub fn worker_count(&self) -> usize {
        self.pool.worker_count()
    }
}

#[async_trait]
impl ScriptExecutor for PhpSapiExecutor {
    async fn execute(&self, request: ScriptRequest) -> Result<ScriptResponse, ExecutorError> {
        self.pool
            .execute_request(request)
            .await
            .map_err(ExecutorError::from)
    }

    fn name(&self) -> &'static str {
        "php-sapi"
    }

    fn shutdown(&self) {
        self.pool.shutdown();
    }
}
