//! PHP executor using pure Rust SAPI implementation.
//!
//! This executor uses the "tokio" SAPI implemented in pure Rust (`src/sapi/`),
//! eliminating the need for the C-based tokio_sapi extension.
//!
//! Features:
//! - Pure Rust SAPI callbacks (no C extension dependency)
//! - Direct `php_execute_script()` execution
//! - Thread-local request context management
//! - SSE streaming support via SAPI ub_write callback

use std::ffi::{c_char, c_int, c_void, CString};
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;

use super::common::{HeartbeatContext, WorkerPool, WorkerRequest};
use super::{ExecuteResult, ExecutorError, ScriptExecutor};
use crate::bridge;
#[cfg(feature = "debug-profile")]
use crate::profiler::ProfileData;
use crate::sapi as rust_sapi; // The pure Rust SAPI module
use crate::sapi::ffi::{
    php_execute_script, php_request_shutdown, php_request_startup, ts_resource_ex,
    zend_destroy_file_handle, zend_file_handle, zend_stream_init_filename,
};
use crate::server::response::StreamChunk;
use crate::types::{ScriptRequest, ScriptResponse};
use rust_sapi::ResponseChunk;

// tokio_sapi extension functions for superglobals
extern "C" {
    fn tokio_sapi_set_files_var(
        field: *const c_char,
        field_len: usize,
        name: *const c_char,
        file_type: *const c_char,
        tmp_name: *const c_char,
        error: c_int,
        size: usize,
    );

    // Batch API for superglobals
    fn tokio_sapi_set_get_vars_batch(
        buffer: *const c_char,
        buffer_len: usize,
        count: usize,
    ) -> c_int;
    fn tokio_sapi_set_post_vars_batch(
        buffer: *const c_char,
        buffer_len: usize,
        count: usize,
    ) -> c_int;
    fn tokio_sapi_set_cookie_vars_batch(
        buffer: *const c_char,
        buffer_len: usize,
        count: usize,
    ) -> c_int;

    fn tokio_sapi_init_superglobals(); // Initialize superglobal array caches
    fn tokio_sapi_build_request(); // Build $_REQUEST from $_GET + $_POST + $_COOKIE
    fn tokio_sapi_request_shutdown(); // Reset superglobal caches (prevents use-after-free)
}

// =============================================================================
// Batch Buffer Helper
// =============================================================================

// Thread-local buffers for batch serialization ($_GET, $_POST, $_COOKIE)
thread_local! {
    static GET_BUFFER: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    static POST_BUFFER: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    static COOKIE_BUFFER: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Pack key-value pairs into a buffer. Returns (buffer_len, count)
#[inline]
fn pack_into_buffer<'a, K, V>(
    buf: &mut Vec<u8>,
    pairs: impl Iterator<Item = (&'a K, &'a V)>,
) -> (usize, usize)
where
    K: AsRef<str> + 'a,
    V: AsRef<str> + 'a,
{
    buf.clear();
    let mut count = 0;

    for (key, value) in pairs {
        let key = key.as_ref();
        let value = value.as_ref();
        buf.extend_from_slice(&((key.len() + 1) as u32).to_le_bytes());
        buf.extend_from_slice(key.as_bytes());
        buf.push(0);
        buf.extend_from_slice(&(value.len() as u32).to_le_bytes());
        buf.extend_from_slice(value.as_bytes());
        count += 1;
    }

    (buf.len(), count)
}

// Heartbeat callback from bridge
extern "C" fn sapi_heartbeat_callback(ctx: *mut c_void, secs: u64) -> i64 {
    if ctx.is_null() {
        return 0;
    }

    // SAFETY: ctx was created from Arc::as_ptr in worker_main_loop
    let heartbeat_ctx = unsafe { &*(ctx as *const HeartbeatContext) };
    if heartbeat_ctx.heartbeat(secs) {
        secs as i64
    } else {
        0
    }
}

// Stream finish callback for tokio_finish_request()
extern "C" fn stream_finish_callback(_ctx: *mut c_void) {
    rust_sapi::mark_stream_finished();
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
struct SapiExecutionTiming {
    superglobals_us: u64,
    script_exec_us: u64,
    #[allow(dead_code)]
    finalize_us: u64,
}

// =============================================================================
// Worker Main Loop
// =============================================================================

fn sapi_worker_main_loop(id: usize, rx: Arc<Mutex<mpsc::Receiver<WorkerRequest>>>) {
    // Initialize thread-local storage for ZTS
    unsafe {
        let _ = ts_resource_ex(0, ptr::null_mut());
    }

    tracing::debug!("SapiWorker {}: Thread-local storage initialized", id);

    loop {
        let work = {
            let guard = rx.lock().unwrap();
            guard.recv()
        };

        match work {
            Ok(WorkerRequest {
                request,
                stream_tx,
                queued_at,
                heartbeat_ctx,
            }) => {
                let request_id = next_request_id();
                let profiling = request.profile;

                // Profiling: queue wait time
                let queue_wait_us = if profiling {
                    queued_at.elapsed().as_micros() as u64
                } else {
                    0
                };

                // Build extended server_vars with TOKIO_* variables
                let req_id_str = request_id.to_string();
                let worker_id_str = id.to_string();
                let mut extended_server_vars: Vec<(String, String)> = request
                    .server_vars
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();
                extended_server_vars.push(("TOKIO_REQUEST_ID".to_string(), req_id_str.clone()));
                extended_server_vars.push(("TOKIO_WORKER_ID".to_string(), worker_id_str.clone()));
                extended_server_vars.push((
                    "TOKIO_SERVER_BUILD_VERSION".to_string(),
                    crate::VERSION.to_string(),
                ));

                // Add trace context to server vars
                extended_server_vars.push(("TRACE_ID".to_string(), request.trace_id.clone()));
                extended_server_vars.push(("SPAN_ID".to_string(), request.span_id.clone()));

                // Convert cookies to (String, String)
                let cookies: Vec<(String, String)> = request
                    .cookies
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();

                // Initialize request context for SAPI callbacks (before php_request_startup)
                rust_sapi::init_context(request_id, id as u64);
                rust_sapi::set_request_data(
                    extended_server_vars,
                    cookies,
                    request.raw_body.as_deref(),
                );

                // Initialize SAPI timing for profiling
                rust_sapi::init_sapi_timing(profiling);

                // Initialize streaming state using executor/sapi infrastructure
                rust_sapi::init_stream_state(stream_tx.clone());

                // Extract request metadata from server_vars for sapi_globals.request_info
                // This MUST be set BEFORE php_request_startup() for $_GET/$_POST parsing
                let find_server_var = |name: &str| -> Option<&str> {
                    request
                        .server_vars
                        .iter()
                        .find(|(k, _)| k.as_ref() == name)
                        .map(|(_, v)| v.as_ref())
                };

                let method_cstr = find_server_var("REQUEST_METHOD")
                    .and_then(|s| CString::new(s).ok())
                    .unwrap_or_else(|| CString::new("GET").unwrap());
                let query_cstr = find_server_var("QUERY_STRING").and_then(|s| CString::new(s).ok());
                let uri_cstr = find_server_var("REQUEST_URI")
                    .and_then(|s| CString::new(s).ok())
                    .unwrap_or_else(|| CString::new("/").unwrap());
                let content_type_cstr =
                    find_server_var("CONTENT_TYPE").and_then(|s| CString::new(s).ok());
                let content_length = find_server_var("CONTENT_LENGTH")
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0);

                // Set request_info in sapi_globals (required for $_GET, $_POST parsing)
                unsafe {
                    rust_sapi::set_request_info(
                        method_cstr.as_ptr(),
                        query_cstr
                            .as_ref()
                            .map(|c| c.as_ptr() as *mut c_char)
                            .unwrap_or(ptr::null_mut()),
                        uri_cstr.as_ptr() as *mut c_char,
                        content_type_cstr
                            .as_ref()
                            .map(|c| c.as_ptr())
                            .unwrap_or(ptr::null()),
                        content_length,
                    );
                }

                // Profiling: PHP startup
                let startup_start = Instant::now();

                // Start PHP request - SAPI callbacks populate $_SERVER
                let startup_ok = unsafe { php_request_startup() } == 0;

                let php_startup_us = if profiling {
                    startup_start.elapsed().as_micros() as u64
                } else {
                    0
                };

                if startup_ok {
                    // Initialize bridge context
                    bridge::init_ctx(request_id, id as u64);
                    bridge::set_request_time(request.received_at);

                    // Set up heartbeat callback via bridge
                    if let Some(ref ctx) = heartbeat_ctx {
                        let ctx_ptr = Arc::as_ptr(ctx) as *mut c_void;
                        unsafe {
                            bridge::set_heartbeat(
                                ctx_ptr,
                                ctx.max_extension(),
                                sapi_heartbeat_callback,
                            );
                        }
                    }

                    // Set up stream finish callback
                    unsafe {
                        bridge::set_stream_finish_callback(ptr::null_mut(), stream_finish_callback);
                    }

                    // Execute script
                    let exec_timing = execute_script(&request, profiling);

                    // Profiling: PHP shutdown
                    let shutdown_start = Instant::now();

                    unsafe {
                        php_request_shutdown(ptr::null_mut());
                        // CRITICAL: Reset superglobal caches to prevent use-after-free
                        // After php_request_shutdown(), PHP's symbol table is destroyed
                        // and cached pointers in C extension become dangling.
                        tokio_sapi_request_shutdown();
                    }

                    let php_shutdown_us = if profiling {
                        shutdown_start.elapsed().as_micros() as u64
                    } else {
                        0
                    };

                    // Destroy bridge context
                    bridge::destroy_ctx();

                    // Send profile data (only with debug-profile feature)
                    #[cfg(feature = "debug-profile")]
                    if profiling {
                        let profile = build_profile_data(
                            queue_wait_us,
                            php_startup_us,
                            &exec_timing,
                            php_shutdown_us,
                        );
                        let _ = stream_tx.blocking_send(ResponseChunk::Profile(Box::new(profile)));
                    }

                    #[cfg(not(feature = "debug-profile"))]
                    let _ = (
                        profiling,
                        queue_wait_us,
                        php_startup_us,
                        &exec_timing,
                        php_shutdown_us,
                    );
                } else {
                    // Send error if startup failed
                    let _ = stream_tx.blocking_send(ResponseChunk::Error(
                        "Failed to start PHP request".to_string(),
                    ));
                }

                // Finalize streaming and cleanup
                rust_sapi::finalize_stream();
                rust_sapi::clear_context();
                rust_sapi::clear_sapi_timing();
            }
            Err(_) => {
                break;
            }
        }
    }

    tracing::debug!("SapiWorker {}: Shutdown complete", id);
}

/// Execute PHP script using php_execute_script()
fn execute_script(request: &ScriptRequest, profiling: bool) -> SapiExecutionTiming {
    let mut timing = SapiExecutionTiming::default();

    // Set superglobals via FFI (same approach as ExtExecutor)
    let phase_start = Instant::now();

    // Initialize superglobal array caches (for $_GET, $_POST, $_FILES)
    unsafe {
        tokio_sapi_init_superglobals();
    }

    // Set $_GET variables (batch)
    let (buf_len, count) = GET_BUFFER.with(|buf| {
        let mut buf = buf.borrow_mut();
        pack_into_buffer(&mut buf, request.get_params.iter().map(|(k, v)| (k, v)))
    });
    if count > 0 {
        GET_BUFFER.with(|buf| unsafe {
            tokio_sapi_set_get_vars_batch(buf.borrow().as_ptr() as *const c_char, buf_len, count);
        });
    }

    // Set $_POST variables (batch)
    let (buf_len, count) = POST_BUFFER.with(|buf| {
        let mut buf = buf.borrow_mut();
        pack_into_buffer(&mut buf, request.post_params.iter().map(|(k, v)| (k, v)))
    });
    if count > 0 {
        POST_BUFFER.with(|buf| unsafe {
            tokio_sapi_set_post_vars_batch(buf.borrow().as_ptr() as *const c_char, buf_len, count);
        });
    }

    // Set $_COOKIE variables (batch)
    // Note: SAPI read_cookies callback is not called by PHP embed SAPI, so we use FFI
    let (buf_len, count) = COOKIE_BUFFER.with(|buf| {
        let mut buf = buf.borrow_mut();
        pack_into_buffer(&mut buf, request.cookies.iter().map(|(k, v)| (k, v)))
    });
    if count > 0 {
        COOKIE_BUFFER.with(|buf| unsafe {
            tokio_sapi_set_cookie_vars_batch(
                buf.borrow().as_ptr() as *const c_char,
                buf_len,
                count,
            );
        });
    }

    // Set $_FILES variables
    tracing::debug!(
        files_entries = request.files.len(),
        "sapi_execute_script: setting $_FILES"
    );
    unsafe {
        for (field_name, files_vec) in &request.files {
            for file in files_vec {
                tracing::debug!(
                    field_name = %field_name,
                    file_name = %file.name,
                    tmp_name = %file.tmp_name,
                    size = file.size,
                    error = file.error,
                    "sapi_execute_script: calling tokio_sapi_set_files_var"
                );
                let name_c = CString::new(file.name.as_str()).unwrap_or_default();
                let type_c = CString::new(file.mime_type.as_str()).unwrap_or_default();
                let tmp_c = CString::new(file.tmp_name.as_str()).unwrap_or_default();

                tokio_sapi_set_files_var(
                    field_name.as_ptr() as *const c_char,
                    field_name.len(),
                    name_c.as_ptr(),
                    type_c.as_ptr(),
                    tmp_c.as_ptr(),
                    file.error as c_int,
                    file.size as usize,
                );
                // Register temp file for cleanup after request
                if !file.tmp_name.is_empty() {
                    rust_sapi::register_temp_file(PathBuf::from(file.tmp_name.as_str()));
                }
            }
        }
    }

    // Build $_REQUEST from $_GET + $_POST
    unsafe {
        tokio_sapi_build_request();
    }

    if profiling {
        timing.superglobals_us = phase_start.elapsed().as_micros() as u64;
    }

    // Execute script via php_execute_script
    let script_start = Instant::now();
    unsafe {
        // OPcache requires absolute/canonical paths for consistent cache keys
        let path = std::path::Path::new(request.script_path.as_str());
        let abs_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let path_str = abs_path.to_string_lossy();

        let path_c = match CString::new(path_str.as_ref()) {
            Ok(c) => c,
            Err(_) => return timing,
        };

        let mut file_handle: zend_file_handle = std::mem::zeroed();
        zend_stream_init_filename(&mut file_handle, path_c.as_ptr());

        // CRITICAL: primary_script must be true for OPcache to cache this script
        file_handle.primary_script = true;

        let _result = php_execute_script(&mut file_handle);

        zend_destroy_file_handle(&mut file_handle);
    }

    if profiling {
        timing.script_exec_us = script_start.elapsed().as_micros() as u64;
    }

    timing
}

#[cfg(feature = "debug-profile")]
fn build_profile_data(
    queue_wait_us: u64,
    php_startup_us: u64,
    exec_timing: &SapiExecutionTiming,
    php_shutdown_us: u64,
) -> ProfileData {
    // Collect SAPI callback timing
    let sapi_timing = rust_sapi::get_sapi_timing();

    ProfileData {
        total_us: queue_wait_us
            + php_startup_us
            + exec_timing.superglobals_us
            + exec_timing.script_exec_us
            + php_shutdown_us,
        queue_wait_us,
        php_startup_us,
        superglobals_us: exec_timing.superglobals_us,
        script_exec_us: exec_timing.script_exec_us,
        php_shutdown_us,
        // SAPI callback timing
        sapi_ub_write_us: sapi_timing.ub_write_us,
        sapi_ub_write_count: sapi_timing.ub_write_count,
        sapi_ub_write_bytes: sapi_timing.ub_write_bytes,
        sapi_header_handler_us: sapi_timing.header_handler_us,
        sapi_header_handler_count: sapi_timing.header_handler_count,
        sapi_send_headers_us: sapi_timing.send_headers_us,
        sapi_flush_us: sapi_timing.flush_us,
        sapi_flush_count: sapi_timing.flush_count,
        sapi_read_post_us: sapi_timing.read_post_us,
        sapi_read_post_bytes: sapi_timing.read_post_bytes,
        sapi_activate_us: sapi_timing.activate_us,
        sapi_deactivate_us: sapi_timing.deactivate_us,
        stream_chunk_count: sapi_timing.stream_chunk_count,
        stream_chunk_bytes: sapi_timing.stream_chunk_bytes,
        context_init_us: sapi_timing.context_init_us,
        context_cleanup_us: sapi_timing.context_cleanup_us,
        ..Default::default()
    }
}

// =============================================================================
// SapiPool
// =============================================================================

struct SapiPool {
    pool: WorkerPool,
}

impl SapiPool {
    fn new(num_workers: usize, queue_capacity: usize) -> Result<Self, String> {
        // Initialize the pure Rust SAPI
        rust_sapi::init()?;

        let pool = if queue_capacity > 0 {
            WorkerPool::with_queue_capacity(num_workers, "sapi", queue_capacity, |id, rx| {
                sapi_worker_main_loop(id, rx);
            })?
        } else {
            WorkerPool::new(num_workers, "sapi", |id, rx| {
                sapi_worker_main_loop(id, rx);
            })?
        };

        tracing::info!(
            "SapiPool initialized with {} workers, queue capacity {} (pure Rust SAPI)",
            num_workers,
            pool.queue_capacity()
        );

        Ok(Self { pool })
    }

    async fn execute_request(&self, request: ScriptRequest) -> Result<ScriptResponse, String> {
        self.pool.execute(request).await
    }

    #[allow(deprecated)]
    fn execute_streaming_request(
        &self,
        request: ScriptRequest,
        buffer_size: usize,
    ) -> Result<tokio::sync::mpsc::Receiver<StreamChunk>, String> {
        self.pool.execute_streaming(request, buffer_size)
    }

    async fn execute_with_auto_sse_request(
        &self,
        request: ScriptRequest,
    ) -> Result<ExecuteResult, String> {
        self.pool.execute_with_auto_sse(request).await
    }

    fn worker_count(&self) -> usize {
        self.pool.worker_count()
    }
}

impl Drop for SapiPool {
    fn drop(&mut self) {
        self.pool.join_all();
        rust_sapi::shutdown();
    }
}

// =============================================================================
// Public Executor Interface
// =============================================================================

/// PHP executor using pure Rust SAPI implementation.
///
/// This executor provides PHP script execution without requiring the C-based
/// tokio_sapi extension. It uses the "tokio" SAPI implemented in pure Rust.
///
/// # Features
/// - No C extension dependency
/// - SAPI callbacks implemented in Rust
/// - SSE streaming support
/// - Request context via thread-local storage
pub struct SapiExecutor {
    pool: SapiPool,
}

impl SapiExecutor {
    /// Creates a new SapiExecutor with custom queue capacity.
    /// If queue_capacity is 0, uses default (workers * 100).
    pub fn with_queue_capacity(
        num_workers: usize,
        queue_capacity: usize,
    ) -> Result<Self, ExecutorError> {
        let pool = SapiPool::new(num_workers, queue_capacity)?;
        Ok(Self { pool })
    }

    /// Returns the number of worker threads.
    pub fn worker_count(&self) -> usize {
        self.pool.worker_count()
    }
}

#[async_trait]
impl ScriptExecutor for SapiExecutor {
    async fn execute(&self, request: ScriptRequest) -> Result<ScriptResponse, ExecutorError> {
        self.pool
            .execute_request(request)
            .await
            .map_err(ExecutorError::from)
    }

    async fn execute_streaming(
        &self,
        request: ScriptRequest,
        buffer_size: usize,
    ) -> Result<tokio::sync::mpsc::Receiver<StreamChunk>, ExecutorError> {
        self.pool
            .execute_streaming_request(request, buffer_size)
            .map_err(ExecutorError::from)
    }

    async fn execute_with_auto_sse(
        &self,
        request: ScriptRequest,
    ) -> Result<ExecuteResult, ExecutorError> {
        self.pool
            .execute_with_auto_sse_request(request)
            .await
            .map_err(ExecutorError::from)
    }

    fn name(&self) -> &'static str {
        "sapi"
    }

    fn shutdown(&self) {
        // Pool shutdown handled by Drop
    }
}
