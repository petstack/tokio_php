//! Common utilities shared between PHP executors.
//!
//! This module contains shared code extracted from php.rs and php_sapi.rs
//! to eliminate duplication and follow DRY principles.

use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;
use tokio::sync::oneshot;

use crate::profiler::{self, ProfileData};
use crate::types::{ScriptRequest, ScriptResponse};

// =============================================================================
// FFI Bindings (shared)
// =============================================================================

#[repr(C)]
pub struct ZendFileHandle {
    pub _data: [u8; 128],
}

#[link(name = "php")]
extern "C" {
    pub fn php_request_startup() -> c_int;
    pub fn php_request_shutdown(dummy: *mut c_void);
    pub fn zend_eval_string(str: *mut c_char, retval: *mut c_void, name: *mut c_char) -> c_int;
    pub fn ts_resource_ex(id: c_int, th_id: *mut c_void) -> *mut c_void;
}

// =============================================================================
// Constants
// =============================================================================

/// PHP code to finalize output: flush buffers, output headers
pub static FINALIZE_CODE: &[u8] = b"while(ob_get_level())ob_end_flush();echo\"\\n---@TOKIO_PHP_HDR@---\\n\";if(($__c=http_response_code())&&$__c!=200)echo\"Status:$__c\\n\";foreach(headers_list()as$h)echo$h.\"\\n\";\0";
pub static FINALIZE_NAME: &[u8] = b"f\0";

/// Delimiter between body and headers in captured output
pub const HEADER_DELIMITER: &str = "\n---@TOKIO_PHP_HDR@---\n";

/// Name for memfd
pub static MEMFD_NAME: &[u8] = b"php_out\0";

// =============================================================================
// Thread-local storage
// =============================================================================

thread_local! {
    /// Reusable buffer for reading PHP output (avoids allocation per request)
    pub static OUTPUT_BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

// =============================================================================
// Worker Pool Infrastructure
// =============================================================================

/// Request sent to a worker thread
pub struct WorkerRequest {
    pub request: ScriptRequest,
    pub response_tx: oneshot::Sender<Result<ScriptResponse, String>>,
    pub queued_at: Instant,
}

/// Handle to a worker thread
pub struct WorkerThread {
    pub handle: JoinHandle<()>,
}

/// Generic worker pool for PHP execution
pub struct WorkerPool {
    request_tx: mpsc::Sender<WorkerRequest>,
    workers: Vec<WorkerThread>,
    worker_count: AtomicUsize,
}

impl WorkerPool {
    /// Creates a new worker pool with the given number of workers.
    /// The `worker_fn` is called for each worker thread.
    pub fn new<F>(num_workers: usize, name_prefix: &str, worker_fn: F) -> Result<Self, String>
    where
        F: Fn(usize, Arc<Mutex<mpsc::Receiver<WorkerRequest>>>) + Send + Clone + 'static,
    {
        let (request_tx, request_rx) = mpsc::channel::<WorkerRequest>();
        let request_rx = Arc::new(Mutex::new(request_rx));

        let mut workers = Vec::with_capacity(num_workers);

        for id in 0..num_workers {
            let rx = Arc::clone(&request_rx);
            let worker_fn = worker_fn.clone();
            let thread_name = format!("{}-{}", name_prefix, id);

            let handle = thread::Builder::new()
                .name(thread_name)
                .spawn(move || {
                    worker_fn(id, rx);
                })
                .map_err(|e| format!("Failed to spawn worker thread {}: {}", id, e))?;

            workers.push(WorkerThread { handle });
        }

        Ok(Self {
            request_tx,
            workers,
            worker_count: AtomicUsize::new(num_workers),
        })
    }

    /// Executes a request asynchronously via the worker pool
    pub async fn execute(&self, request: ScriptRequest) -> Result<ScriptResponse, String> {
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

    /// Returns the number of workers
    pub fn worker_count(&self) -> usize {
        self.worker_count.load(Ordering::Relaxed)
    }

    /// Waits for all workers to finish
    pub fn join_all(&mut self) {
        for worker in self.workers.drain(..) {
            let _ = worker.handle.join();
        }
    }
}

// =============================================================================
// PHP Code Generation
// =============================================================================

/// Checks if a string needs PHP escaping
#[inline]
pub fn needs_escape(s: &str) -> bool {
    s.bytes().any(|b| b == b'\\' || b == b'\'' || b == 0)
}

/// Writes a PHP-escaped string to a buffer (zero-alloc for clean strings)
#[inline]
pub fn write_escaped(buf: &mut String, s: &str) {
    if !needs_escape(s) {
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

/// Writes a PHP key-value pair: 'key'=>'value'
#[inline]
pub fn write_kv(buf: &mut String, key: &str, value: &str) {
    buf.push('\'');
    write_escaped(buf, key);
    buf.push_str("'=>'");
    write_escaped(buf, value);
    buf.push('\'');
}

/// Builds PHP code to set superglobals ($_GET, $_POST, $_SERVER, etc.)
pub fn build_superglobals_code(request: &ScriptRequest) -> String {
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
        write_kv(&mut code, key, value);
    }
    code.push_str("];");

    // $_POST
    code.push_str("$_POST=[");
    for (i, (key, value)) in request.post_params.iter().enumerate() {
        if i > 0 { code.push(','); }
        write_kv(&mut code, key, value);
    }
    code.push_str("];");

    // $_SERVER
    code.push_str("$_SERVER=[");
    for (i, (key, value)) in request.server_vars.iter().enumerate() {
        if i > 0 { code.push(','); }
        write_kv(&mut code, key, value);
    }
    code.push_str("];");

    // $_COOKIE
    code.push_str("$_COOKIE=[");
    for (i, (key, value)) in request.cookies.iter().enumerate() {
        if i > 0 { code.push(','); }
        write_kv(&mut code, key, value);
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
            write_escaped(&mut code, field_name);
            code.push_str("'=>");

            if files_vec.len() == 1 {
                let f = &files_vec[0];
                code.push_str("['name'=>'");
                write_escaped(&mut code, &f.name);
                code.push_str("','type'=>'");
                write_escaped(&mut code, &f.mime_type);
                code.push_str("','tmp_name'=>'");
                write_escaped(&mut code, &f.tmp_name);
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
                    write_escaped(&mut code, &f.name);
                    code.push('\'');
                }
                code.push_str("],'type'=>[");
                for (j, f) in files_vec.iter().enumerate() {
                    if j > 0 { code.push(','); }
                    code.push('\'');
                    write_escaped(&mut code, &f.mime_type);
                    code.push('\'');
                }
                code.push_str("],'tmp_name'=>[");
                for (j, f) in files_vec.iter().enumerate() {
                    if j > 0 { code.push(','); }
                    code.push('\'');
                    write_escaped(&mut code, &f.tmp_name);
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

/// Builds combined code: superglobals + require script (single eval)
pub fn build_combined_code(request: &ScriptRequest) -> String {
    let mut code = build_superglobals_code(request);
    code.push_str("require'");
    write_escaped(&mut code, &request.script_path);
    code.push_str("';");
    code
}

// =============================================================================
// Output Parsing
// =============================================================================

/// Parses headers from the captured output string
pub fn parse_headers_str(headers_str: &str) -> Vec<(String, String)> {
    headers_str
        .lines()
        .filter_map(|line| {
            line.split_once(':').map(|(name, value)| {
                (name.trim().to_string(), value.trim().to_string())
            })
        })
        .collect()
}

// =============================================================================
// PHP Execution
// =============================================================================

/// Timing data for profiling
#[derive(Default)]
pub struct ExecutionTiming {
    pub superglobals_build_us: u64,
    pub memfd_setup_us: u64,
    pub script_exec_us: u64,
    pub finalize_eval_us: u64,
    pub stdout_restore_us: u64,
    pub output_read_us: u64,
    pub output_parse_us: u64,
}

/// Executes PHP script with the given request, capturing output via memfd.
/// Returns the response body, headers, and optional timing data.
pub fn execute_php_script(
    request: &ScriptRequest,
    profiling: bool,
    queue_wait_us: u64,
    php_startup_us: u64,
) -> Result<ScriptResponse, String> {
    let mut timing = ExecutionTiming::default();

    // Build combined code: superglobals + require script
    let build_start = Instant::now();
    let combined_code = build_combined_code(request);
    if profiling {
        timing.superglobals_build_us = build_start.elapsed().as_micros() as u64;
    }

    // Set up memfd for stdout capture (Linux-specific, uses tmpfile on other platforms)
    let memfd_start = Instant::now();
    #[cfg(target_os = "linux")]
    let write_fd = unsafe {
        libc::syscall(
            libc::SYS_memfd_create,
            MEMFD_NAME.as_ptr(),
            0 as libc::c_uint,
        ) as libc::c_int
    };
    #[cfg(not(target_os = "linux"))]
    let write_fd = unsafe {
        // Fallback for non-Linux (e.g., macOS) - use tmpfile
        let f = libc::tmpfile();
        if f.is_null() {
            -1
        } else {
            libc::fileno(f)
        }
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
    if profiling {
        timing.memfd_setup_us = memfd_start.elapsed().as_micros() as u64;
    }

    // Execute combined code in single eval
    let script_start = Instant::now();
    unsafe {
        let code_c = CString::new(combined_code).map_err(|e| e.to_string())?;
        let name_c = CString::new("x").unwrap();

        zend_eval_string(
            code_c.as_ptr() as *mut c_char,
            ptr::null_mut(),
            name_c.as_ptr() as *mut c_char,
        );
    }
    if profiling {
        timing.script_exec_us = script_start.elapsed().as_micros() as u64;
    }

    // Flush output buffers and capture headers
    let finalize_start = Instant::now();
    unsafe {
        zend_eval_string(
            FINALIZE_CODE.as_ptr() as *mut c_char,
            ptr::null_mut(),
            FINALIZE_NAME.as_ptr() as *mut c_char,
        );
    }
    if profiling {
        timing.finalize_eval_us = finalize_start.elapsed().as_micros() as u64;
    }

    // Restore stdout
    let restore_start = Instant::now();
    unsafe {
        libc::fflush(ptr::null_mut());
        libc::dup2(original_stdout, 1);
        libc::close(original_stdout);
    }
    if profiling {
        timing.stdout_restore_us = restore_start.elapsed().as_micros() as u64;
    }

    // Read output from memfd
    let read_start = Instant::now();
    let raw_output = OUTPUT_BUFFER.with(|buf| {
        let mut buf = buf.borrow_mut();
        buf.clear();

        unsafe {
            libc::lseek(write_fd, 0, libc::SEEK_SET);

            let mut chunk = [0u8; 8192];
            loop {
                let n = libc::read(write_fd, chunk.as_mut_ptr() as *mut libc::c_void, chunk.len());
                if n <= 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n as usize]);
            }

            libc::close(write_fd);
        }

        String::from_utf8_lossy(&buf).into_owned()
    });
    if profiling {
        timing.output_read_us = read_start.elapsed().as_micros() as u64;
    }

    // Parse body and headers
    let parse_start = Instant::now();
    let (body, headers) = if let Some(pos) = raw_output.find(HEADER_DELIMITER) {
        let body = raw_output[..pos].to_string();
        let headers_str = &raw_output[pos + HEADER_DELIMITER.len()..];
        let headers = parse_headers_str(headers_str);
        (body, headers)
    } else {
        (raw_output, Vec::new())
    };
    if profiling {
        timing.output_parse_us = parse_start.elapsed().as_micros() as u64;
    }

    let output_capture_us = timing.finalize_eval_us + timing.stdout_restore_us
        + timing.output_read_us + timing.output_parse_us;
    let superglobals_us = timing.superglobals_build_us;

    // Build profile data if profiling
    let profile = if profiling {
        Some(ProfileData {
            total_us: 0, // Filled by caller after shutdown
            parse_request_us: 0,
            headers_extract_us: 0,
            query_parse_us: 0,
            cookies_parse_us: 0,
            body_read_us: 0,
            body_parse_us: 0,
            server_vars_us: 0,
            path_resolve_us: 0,
            file_check_us: 0,
            queue_wait_us,
            php_startup_us,
            superglobals_us,
            superglobals_build_us: timing.superglobals_build_us,
            superglobals_eval_us: 0, // Part of script_exec_us now
            memfd_setup_us: timing.memfd_setup_us,
            script_exec_us: timing.script_exec_us,
            output_capture_us,
            finalize_eval_us: timing.finalize_eval_us,
            stdout_restore_us: timing.stdout_restore_us,
            output_read_us: timing.output_read_us,
            output_parse_us: timing.output_parse_us,
            php_shutdown_us: 0, // Filled by caller
            response_build_us: 0,
            ..Default::default()
        })
    } else {
        None
    };

    Ok(ScriptResponse { body, headers, profile })
}

/// Worker thread main loop - processes requests until channel closes
pub fn worker_main_loop(
    id: usize,
    rx: Arc<Mutex<mpsc::Receiver<WorkerRequest>>>,
) {
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

                // Measure queue wait time
                let queue_wait_us = if profiling {
                    queued_at.elapsed().as_micros() as u64
                } else {
                    0
                };

                // Start PHP request
                let startup_start = Instant::now();
                let startup_ok = unsafe { php_request_startup() } == 0;
                let php_startup_us = if profiling {
                    startup_start.elapsed().as_micros() as u64
                } else {
                    0
                };

                let result = if startup_ok {
                    let res = execute_php_script(&request, profiling, queue_wait_us, php_startup_us);

                    let shutdown_start = Instant::now();
                    unsafe { php_request_shutdown(ptr::null_mut()); }

                    // Add shutdown time to profile
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
