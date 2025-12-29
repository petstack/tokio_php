//! PHP executor using tokio_sapi extension for FFI superglobals.
//!
//! This executor uses the tokio_sapi PHP extension to set superglobals directly
//! via FFI calls, bypassing zend_eval_string() overhead for better performance.
//!
//! Features:
//! - Request ID tracking via tokio_sapi_request_init()
//! - FFI superglobals: $_GET, $_POST, $_SERVER, $_COOKIE, $_FILES, $_REQUEST
//! - Script execution via tokio_sapi_execute_script()

use std::ffi::{c_char, c_int, c_void, CString};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;

use super::common::{
    php_request_shutdown, php_request_startup, ts_resource_ex,
    StdoutCapture, WorkerPool, WorkerRequest,
    FINALIZE_CODE, FINALIZE_NAME,
};
use super::sapi;
use super::{ExecutorError, ScriptExecutor};
use crate::profiler::{self, ProfileData};
use crate::types::{ScriptRequest, ScriptResponse};

// =============================================================================
// FFI Bindings
// =============================================================================

#[link(name = "php")]
extern "C" {
    fn zend_eval_string(str: *mut c_char, retval: *mut c_void, name: *mut c_char) -> c_int;
}

// tokio_sapi extension functions (from static library)
extern "C" {
    fn tokio_sapi_request_init(request_id: u64) -> c_int;
    fn tokio_sapi_request_shutdown();

    // Superglobal setters - direct access without eval!
    fn tokio_sapi_set_server_var(
        key: *const c_char, key_len: usize,
        value: *const c_char, value_len: usize
    );
    fn tokio_sapi_set_get_var(
        key: *const c_char, key_len: usize,
        value: *const c_char, value_len: usize
    );
    fn tokio_sapi_set_post_var(
        key: *const c_char, key_len: usize,
        value: *const c_char, value_len: usize
    );
    fn tokio_sapi_set_cookie_var(
        key: *const c_char, key_len: usize,
        value: *const c_char, value_len: usize
    );
    fn tokio_sapi_set_files_var(
        field: *const c_char, field_len: usize,
        name: *const c_char,
        file_type: *const c_char,
        tmp_name: *const c_char,
        error: c_int,
        size: usize
    );

    // Batch API - set multiple variables in one FFI call
    fn tokio_sapi_set_server_vars_batch(buffer: *const c_char, buffer_len: usize, count: usize) -> c_int;
    fn tokio_sapi_set_get_vars_batch(buffer: *const c_char, buffer_len: usize, count: usize) -> c_int;
    fn tokio_sapi_set_post_vars_batch(buffer: *const c_char, buffer_len: usize, count: usize) -> c_int;
    fn tokio_sapi_set_cookie_vars_batch(buffer: *const c_char, buffer_len: usize, count: usize) -> c_int;

    // Ultra-batch API - set ALL superglobals in one FFI call
    fn tokio_sapi_set_all_superglobals(
        server_buf: *const c_char, server_len: usize, server_count: usize,
        get_buf: *const c_char, get_len: usize, get_count: usize,
        post_buf: *const c_char, post_len: usize, post_count: usize,
        cookie_buf: *const c_char, cookie_len: usize, cookie_count: usize,
    );

    fn tokio_sapi_clear_superglobals();
    fn tokio_sapi_init_superglobals();   // Initialize superglobal array caches (call once per request)
    fn tokio_sapi_init_request_state();  // Replaces header_remove();ob_start() eval
    fn tokio_sapi_build_request();

    // Script execution
    fn tokio_sapi_execute_script(path: *const c_char) -> c_int;
}

// =============================================================================
// Batch Buffer Helper
// =============================================================================

// Thread-local buffers for ultra-batch serialization (one per superglobal type)
thread_local! {
    static SERVER_BUFFER: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    static GET_BUFFER: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    static POST_BUFFER: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    static COOKIE_BUFFER: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Pack key-value pairs into a buffer. Returns (buffer_len, count)
#[inline]
fn pack_into_buffer<'a>(
    buf: &mut Vec<u8>,
    pairs: impl Iterator<Item = (&'a String, &'a String)>,
    extras: &[(&str, &str)],
) -> (usize, usize) {
    buf.clear();
    let mut count = 0;

    for (key, value) in pairs {
        buf.extend_from_slice(&((key.len() + 1) as u32).to_le_bytes());
        buf.extend_from_slice(key.as_bytes());
        buf.push(0);
        buf.extend_from_slice(&(value.len() as u32).to_le_bytes());
        buf.extend_from_slice(value.as_bytes());
        count += 1;
    }

    for (key, value) in extras {
        buf.extend_from_slice(&((key.len() + 1) as u32).to_le_bytes());
        buf.extend_from_slice(key.as_bytes());
        buf.push(0);
        buf.extend_from_slice(&(value.len() as u32).to_le_bytes());
        buf.extend_from_slice(value.as_bytes());
        count += 1;
    }

    (buf.len(), count)
}

// =============================================================================
// Request ID Counter
// =============================================================================

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_request_id() -> u64 {
    REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed)
}

// =============================================================================
// Timing Data
// =============================================================================

#[derive(Default)]
struct ExtExecutionTiming {
    // FFI breakdown
    ffi_request_init_us: u64,
    ffi_clear_us: u64,
    ffi_server_us: u64,
    ffi_server_count: u64,
    ffi_get_us: u64,
    ffi_get_count: u64,
    ffi_post_us: u64,
    ffi_post_count: u64,
    ffi_cookie_us: u64,
    ffi_cookie_count: u64,
    ffi_files_us: u64,
    ffi_files_count: u64,
    ffi_build_request_us: u64,
    ffi_init_eval_us: u64,
    // Total superglobals time (sum of above)
    superglobals_build_us: u64,
    // Other phases
    memfd_setup_us: u64,
    script_exec_us: u64,
    finalize_us: u64,
}

// =============================================================================
// Script Execution (using FFI for superglobals - no eval overhead!)
// =============================================================================

/// Execute PHP script using FFI for superglobals (faster than eval!)
fn execute_script_with_ffi(
    request: &ScriptRequest,
    request_id: u64,
    worker_id: usize,
    profiling: bool,
) -> Result<(StdoutCapture, ExtExecutionTiming), String> {
    let mut timing = ExtExecutionTiming::default();

    // Clear captured headers from SAPI
    sapi::clear_captured_headers();

    // === FFI Superglobals with granular timing ===
    let total_start = Instant::now();

    // 1. Clear superglobals and initialize array caches
    let phase_start = Instant::now();
    unsafe {
        tokio_sapi_clear_superglobals();
        tokio_sapi_init_superglobals();
    }
    if profiling {
        timing.ffi_clear_us = phase_start.elapsed().as_micros() as u64;
    }

    // 2. Set $_SERVER variables (batch)
    let phase_start = Instant::now();
    let req_id_value = request_id.to_string();
    let worker_id_value = worker_id.to_string();
    let (buf_len, count) = SERVER_BUFFER.with(|buf| {
        let mut buf = buf.borrow_mut();
        pack_into_buffer(&mut buf, request.server_vars.iter().map(|(k, v)| (k, v)), &[
            ("TOKIO_REQUEST_ID", &req_id_value),
            ("TOKIO_WORKER_ID", &worker_id_value),
        ])
    });
    if count > 0 {
        SERVER_BUFFER.with(|buf| unsafe {
            tokio_sapi_set_server_vars_batch(buf.borrow().as_ptr() as *const c_char, buf_len, count);
        });
    }
    if profiling {
        timing.ffi_server_us = phase_start.elapsed().as_micros() as u64;
        timing.ffi_server_count = count as u64;
    }

    // 3. Set $_GET variables (batch)
    let phase_start = Instant::now();
    let (buf_len, count) = GET_BUFFER.with(|buf| {
        let mut buf = buf.borrow_mut();
        pack_into_buffer(&mut buf, request.get_params.iter().map(|(k, v)| (k, v)), &[])
    });
    if count > 0 {
        GET_BUFFER.with(|buf| unsafe {
            tokio_sapi_set_get_vars_batch(buf.borrow().as_ptr() as *const c_char, buf_len, count);
        });
    }
    if profiling {
        timing.ffi_get_us = phase_start.elapsed().as_micros() as u64;
        timing.ffi_get_count = count as u64;
    }

    // 4. Set $_POST variables (batch)
    let phase_start = Instant::now();
    let (buf_len, count) = POST_BUFFER.with(|buf| {
        let mut buf = buf.borrow_mut();
        pack_into_buffer(&mut buf, request.post_params.iter().map(|(k, v)| (k, v)), &[])
    });
    if count > 0 {
        POST_BUFFER.with(|buf| unsafe {
            tokio_sapi_set_post_vars_batch(buf.borrow().as_ptr() as *const c_char, buf_len, count);
        });
    }
    if profiling {
        timing.ffi_post_us = phase_start.elapsed().as_micros() as u64;
        timing.ffi_post_count = count as u64;
    }

    // 5. Set $_COOKIE variables (batch)
    let phase_start = Instant::now();
    let (buf_len, count) = COOKIE_BUFFER.with(|buf| {
        let mut buf = buf.borrow_mut();
        pack_into_buffer(&mut buf, request.cookies.iter().map(|(k, v)| (k, v)), &[])
    });
    if count > 0 {
        COOKIE_BUFFER.with(|buf| unsafe {
            tokio_sapi_set_cookie_vars_batch(buf.borrow().as_ptr() as *const c_char, buf_len, count);
        });
    }
    if profiling {
        timing.ffi_cookie_us = phase_start.elapsed().as_micros() as u64;
        timing.ffi_cookie_count = count as u64;
    }

    // 6. Set $_FILES variables
    let phase_start = Instant::now();
    let mut files_count: u64 = 0;
    unsafe {
        for (field_name, files_vec) in &request.files {
            for file in files_vec {
                let name_c = CString::new(file.name.as_str()).unwrap_or_default();
                let type_c = CString::new(file.mime_type.as_str()).unwrap_or_default();
                let tmp_c = CString::new(file.tmp_name.as_str()).unwrap_or_default();

                tokio_sapi_set_files_var(
                    field_name.as_ptr() as *const c_char, field_name.len(),
                    name_c.as_ptr(),
                    type_c.as_ptr(),
                    tmp_c.as_ptr(),
                    file.error as c_int,
                    file.size as usize
                );
                files_count += 1;
            }
        }
    }
    if profiling {
        timing.ffi_files_us = phase_start.elapsed().as_micros() as u64;
        timing.ffi_files_count = files_count;
    }

    // 7. Build $_REQUEST from $_GET + $_POST
    let phase_start = Instant::now();
    unsafe { tokio_sapi_build_request(); }
    if profiling {
        timing.ffi_build_request_us = phase_start.elapsed().as_micros() as u64;
        timing.superglobals_build_us = total_start.elapsed().as_micros() as u64;
    }

    // Set up stdout capture
    let memfd_start = Instant::now();
    let capture = StdoutCapture::new()?;
    if profiling {
        timing.memfd_setup_us = memfd_start.elapsed().as_micros() as u64;
    }

    // Initialize PHP state (headers, output buffering) via FFI
    let init_start = Instant::now();
    unsafe {
        tokio_sapi_init_request_state();
    }
    if profiling {
        timing.ffi_init_eval_us = init_start.elapsed().as_micros() as u64;
    }

    // Execute script via FFI
    let script_start = Instant::now();
    unsafe {
        let path_c = CString::new(request.script_path.as_str()).map_err(|e| e.to_string())?;
        tokio_sapi_execute_script(path_c.as_ptr());
    }
    if profiling {
        timing.script_exec_us = script_start.elapsed().as_micros() as u64;
    }

    // Finalize (flush buffers)
    let finalize_start = Instant::now();
    unsafe {
        zend_eval_string(
            FINALIZE_CODE.as_ptr() as *mut c_char,
            ptr::null_mut(),
            FINALIZE_NAME.as_ptr() as *mut c_char,
        );
    }
    if profiling {
        timing.finalize_us = finalize_start.elapsed().as_micros() as u64;
    }

    Ok((capture, timing))
}

/// Finalize execution and build response
fn finalize_execution(
    capture: StdoutCapture,
    timing: ExtExecutionTiming,
    profiling: bool,
    queue_wait_us: u64,
    php_startup_us: u64,
    php_shutdown_us: u64,
) -> Result<ScriptResponse, String> {
    // Restore stdout and read output
    let restore_start = Instant::now();
    let body = capture.finalize();
    let stdout_restore_us = if profiling {
        restore_start.elapsed().as_micros() as u64
    } else {
        0
    };

    // Get headers from SAPI
    let mut headers = sapi::get_captured_headers();
    let status = sapi::get_captured_status();
    if status != 200 {
        headers.insert(0, ("Status".to_string(), status.to_string()));
    }

    let profile = if profiling {
        Some(ProfileData {
            total_us: queue_wait_us + php_startup_us + timing.superglobals_build_us
                + timing.ffi_init_eval_us + timing.script_exec_us + timing.finalize_us
                + stdout_restore_us + php_shutdown_us,
            queue_wait_us,
            php_startup_us,
            superglobals_us: timing.superglobals_build_us,
            superglobals_build_us: timing.superglobals_build_us,
            superglobals_eval_us: 0,
            // FFI breakdown
            ffi_request_init_us: timing.ffi_request_init_us,
            ffi_clear_us: timing.ffi_clear_us,
            ffi_server_us: timing.ffi_server_us,
            ffi_server_count: timing.ffi_server_count,
            ffi_get_us: timing.ffi_get_us,
            ffi_get_count: timing.ffi_get_count,
            ffi_post_us: timing.ffi_post_us,
            ffi_post_count: timing.ffi_post_count,
            ffi_cookie_us: timing.ffi_cookie_us,
            ffi_cookie_count: timing.ffi_cookie_count,
            ffi_files_us: timing.ffi_files_us,
            ffi_files_count: timing.ffi_files_count,
            ffi_build_request_us: timing.ffi_build_request_us,
            ffi_init_eval_us: timing.ffi_init_eval_us,
            // Other phases
            memfd_setup_us: timing.memfd_setup_us,
            script_exec_us: timing.script_exec_us,
            output_capture_us: timing.finalize_us + stdout_restore_us,
            finalize_eval_us: timing.finalize_us,
            stdout_restore_us,
            output_read_us: 0,
            output_parse_us: 0,
            php_shutdown_us,
            response_build_us: 0,
            ..Default::default()
        })
    } else {
        None
    };

    Ok(ScriptResponse { body, headers, profile })
}

// =============================================================================
// Worker Main Loop
// =============================================================================

fn ext_worker_main_loop(
    id: usize,
    rx: Arc<Mutex<mpsc::Receiver<WorkerRequest>>>,
) {
    // Initialize thread-local storage for ZTS
    unsafe {
        let _ = ts_resource_ex(0, ptr::null_mut());
    }

    tracing::debug!("ExtWorker {}: Thread-local storage initialized", id);

    loop {
        let work = {
            let guard = rx.lock().unwrap();
            guard.recv()
        };

        match work {
            Ok(WorkerRequest { request, response_tx, queued_at }) => {
                let profiling = request.profile && profiler::is_enabled();
                let request_id = next_request_id();

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
                    // Initialize tokio_sapi request context
                    let req_init_start = Instant::now();
                    unsafe {
                        tokio_sapi_request_init(request_id);
                    }
                    let ffi_request_init_us = if profiling {
                        req_init_start.elapsed().as_micros() as u64
                    } else {
                        0
                    };

                    // Execute script with FFI superglobals (no eval overhead!)
                    match execute_script_with_ffi(&request, request_id, id, profiling) {
                        Ok((capture, mut timing)) => {
                            // Add request_init timing
                            timing.ffi_request_init_us = ffi_request_init_us;
                            // Shutdown tokio_sapi and PHP request
                            let shutdown_start = Instant::now();
                            unsafe {
                                tokio_sapi_request_shutdown();
                                php_request_shutdown(ptr::null_mut());
                            }
                            let php_shutdown_us = if profiling {
                                shutdown_start.elapsed().as_micros() as u64
                            } else {
                                0
                            };

                            // Finalize and build response
                            finalize_execution(
                                capture,
                                timing,
                                profiling,
                                queue_wait_us,
                                php_startup_us,
                                php_shutdown_us,
                            )
                        }
                        Err(e) => {
                            unsafe {
                                tokio_sapi_request_shutdown();
                                php_request_shutdown(ptr::null_mut());
                            }
                            Err(e)
                        }
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

    tracing::debug!("ExtWorker {}: Shutdown complete", id);
}

// =============================================================================
// ExtExecutor
// =============================================================================

struct ExtPool {
    pool: WorkerPool,
}

impl ExtPool {
    fn new(num_workers: usize) -> Result<Self, String> {
        Self::with_queue_capacity(num_workers, 0)
    }

    fn with_queue_capacity(num_workers: usize, queue_capacity: usize) -> Result<Self, String> {
        // Initialize SAPI (same as PhpExecutor)
        sapi::init()?;

        let pool = if queue_capacity > 0 {
            WorkerPool::with_queue_capacity(num_workers, "ext", queue_capacity, |id, rx| {
                ext_worker_main_loop(id, rx);
            })?
        } else {
            WorkerPool::new(num_workers, "ext", |id, rx| {
                ext_worker_main_loop(id, rx);
            })?
        };

        for id in 0..num_workers {
            tracing::debug!("Spawned ExtWorker thread {}", id);
        }

        tracing::info!(
            "ExtPool initialized with {} workers, queue capacity {} (FFI superglobals)",
            num_workers,
            pool.queue_capacity()
        );

        Ok(Self { pool })
    }

    async fn execute_request(&self, request: ScriptRequest) -> Result<ScriptResponse, String> {
        self.pool.execute(request).await
    }

    fn worker_count(&self) -> usize {
        self.pool.worker_count()
    }
}

impl Drop for ExtPool {
    fn drop(&mut self) {
        self.pool.join_all();
        sapi::shutdown();
    }
}

// =============================================================================
// Public Executor Interface
// =============================================================================

/// PHP executor using tokio_sapi extension for direct superglobal access.
///
/// This executor provides the same functionality as PhpExecutor but uses
/// FFI calls to set superglobals directly, avoiding zend_eval_string() overhead.
pub struct ExtExecutor {
    pool: ExtPool,
}

impl ExtExecutor {
    /// Creates a new ExtExecutor with the specified number of worker threads.
    /// Uses default queue capacity (workers * 100).
    pub fn new(num_workers: usize) -> Result<Self, ExecutorError> {
        Self::with_queue_capacity(num_workers, 0)
    }

    /// Creates a new ExtExecutor with custom queue capacity.
    /// If queue_capacity is 0, uses default (workers * 100).
    pub fn with_queue_capacity(num_workers: usize, queue_capacity: usize) -> Result<Self, ExecutorError> {
        let pool = ExtPool::with_queue_capacity(num_workers, queue_capacity)?;
        Ok(Self { pool })
    }

    /// Returns the number of worker threads.
    pub fn worker_count(&self) -> usize {
        self.pool.worker_count()
    }
}

#[async_trait]
impl ScriptExecutor for ExtExecutor {
    async fn execute(&self, request: ScriptRequest) -> Result<ScriptResponse, ExecutorError> {
        self.pool
            .execute_request(request)
            .await
            .map_err(ExecutorError::from)
    }

    fn name(&self) -> &'static str {
        "ext"
    }

    fn shutdown(&self) {
        // Pool shutdown handled by Drop
    }
}
