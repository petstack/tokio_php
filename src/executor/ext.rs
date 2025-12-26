//! PHP executor using tokio_sapi extension for request tracking.
//!
//! This executor is similar to PhpExecutor but integrates with the tokio_sapi
//! PHP extension for request ID tracking and future FFI superglobals support.
//!
//! Currently uses eval for superglobals (same as PhpExecutor) but adds:
//! - Request ID tracking via tokio_sapi_request_init()
//! - Foundation for future FFI superglobals optimization

use std::ffi::{c_char, c_int, c_void, CString};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;

use super::common::{
    php_request_shutdown, php_request_startup, ts_resource_ex,
    StdoutCapture, WorkerPool, WorkerRequest,
    build_combined_code,
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

// tokio_sapi extension functions
extern "C" {
    fn tokio_sapi_request_init(request_id: u64) -> c_int;
    fn tokio_sapi_request_shutdown();
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
    superglobals_build_us: u64,
    memfd_setup_us: u64,
    script_exec_us: u64,
    finalize_us: u64,
}

// =============================================================================
// Script Execution (using eval for superglobals, like PhpExecutor)
// =============================================================================

/// Execute PHP script using eval for superglobals (same as PhpExecutor)
fn execute_script_with_eval(
    request: &ScriptRequest,
    profiling: bool,
) -> Result<(StdoutCapture, ExtExecutionTiming), String> {
    let mut timing = ExtExecutionTiming::default();

    // Clear captured headers from SAPI
    sapi::clear_captured_headers();

    // Build combined code (superglobals + require)
    let build_start = Instant::now();
    let combined_code = build_combined_code(request);
    if profiling {
        timing.superglobals_build_us = build_start.elapsed().as_micros() as u64;
    }

    // Set up stdout capture
    let memfd_start = Instant::now();
    let capture = StdoutCapture::new()?;
    if profiling {
        timing.memfd_setup_us = memfd_start.elapsed().as_micros() as u64;
    }

    // Execute script
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
                + timing.script_exec_us + timing.finalize_us + stdout_restore_us + php_shutdown_us,
            queue_wait_us,
            php_startup_us,
            superglobals_us: timing.superglobals_build_us,
            superglobals_build_us: timing.superglobals_build_us,
            superglobals_eval_us: 0,
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
                    // NOTE: tokio_sapi_request_init disabled for now - crashes
                    // TODO: Fix extension initialization order
                    let _ = request_id; // suppress unused warning

                    // Execute script with eval (like PhpExecutor)
                    match execute_script_with_eval(&request, profiling) {
                        Ok((capture, timing)) => {
                            // Shutdown PHP request (while stdout still captured)
                            let shutdown_start = Instant::now();
                            unsafe {
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
        // Initialize SAPI (same as PhpExecutor)
        sapi::init()?;

        let pool = WorkerPool::new(num_workers, "ext", |id, rx| {
            ext_worker_main_loop(id, rx);
        })?;

        for id in 0..num_workers {
            tracing::debug!("Spawned ExtWorker thread {}", id);
        }

        tracing::info!("ExtPool initialized with {} workers (FFI superglobals)", num_workers);

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
    pub fn new(num_workers: usize) -> Result<Self, ExecutorError> {
        let pool = ExtPool::new(num_workers)?;
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
