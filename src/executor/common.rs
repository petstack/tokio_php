//! Common utilities shared between PHP executors.
//!
//! This module contains shared code extracted from php.rs and php_sapi.rs
//! to eliminate duplication and follow DRY principles.

use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CString};
use std::ptr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc as tokio_mpsc, oneshot};

use crate::bridge::{FinishChannel, FinishData, StreamingChannel};
use crate::executor::sapi::{self, ResponseChunk};
use crate::profiler::ProfileData;
use crate::server::response::StreamChunk;
use crate::types::{ScriptRequest, ScriptResponse};

// =============================================================================
// Execute Result Types
// =============================================================================

/// Buffer size for streaming channel in auto-detect mode.
#[allow(dead_code)]
pub const AUTO_SSE_BUFFER_SIZE: usize = 32;

/// Result of execute_with_auto_sse().
///
/// Can be either a normal response or a streaming response (when PHP
/// dynamically enables SSE via Content-Type: text/event-stream header).
pub enum ExecuteResult {
    /// Normal response (no streaming).
    Normal(Box<ScriptResponse>),
    /// Streaming response (SSE auto-detected via Content-Type header).
    /// Contains initial headers, status code, and receiver for stream chunks.
    Streaming {
        headers: Vec<(String, String)>,
        status_code: u16,
        receiver: tokio_mpsc::Receiver<StreamChunk>,
    },
}

// =============================================================================
// FFI Bindings (shared)
// =============================================================================

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

/// PHP code to finalize output - just flush buffers
pub static FINALIZE_CODE: &[u8] = b"1;\0";
pub static FINALIZE_NAME: &[u8] = b"f\0";

/// Name for memfd (Linux only)
#[cfg(target_os = "linux")]
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

/// Request sent to a worker thread.
///
/// All requests now use streaming via `stream_tx`. The worker sends:
/// 1. `ResponseChunk::Headers` - once, when first output occurs or script ends
/// 2. `ResponseChunk::Body(data)` - for each output chunk
/// 3. `ResponseChunk::End` - when script completes or tokio_finish_request() is called
/// 4. `ResponseChunk::Error(msg)` - if execution fails
pub struct WorkerRequest {
    pub request: ScriptRequest,
    /// Channel for streaming response chunks back to the HTTP handler.
    /// Worker uses `blocking_send()` since it runs in a sync context.
    pub stream_tx: tokio_mpsc::Sender<ResponseChunk>,
    #[allow(dead_code)]
    pub queued_at: Instant,
    /// Heartbeat context for timeout extension (shared with async side)
    pub heartbeat_ctx: Option<Arc<HeartbeatContext>>,
}

/// Legacy request struct for backward compatibility during migration.
/// Will be removed after full streaming migration.
#[allow(dead_code)]
pub struct LegacyWorkerRequest {
    pub request: ScriptRequest,
    pub response_tx: oneshot::Sender<Result<ScriptResponse, String>>,
    pub queued_at: Instant,
    pub heartbeat_ctx: Option<Arc<HeartbeatContext>>,
    pub finish_channel: Option<Arc<FinishChannel>>,
    pub streaming_channel: Option<Arc<StreamingChannel>>,
    pub explicit_sse: bool,
}

/// Handle to a worker thread
pub struct WorkerThread {
    pub handle: JoinHandle<()>,
}

/// Default queue capacity multiplier per worker
const DEFAULT_QUEUE_MULTIPLIER: usize = 100;

/// Error returned when queue is full
pub const QUEUE_FULL_ERROR: &str = "Queue full";

/// Error returned when request times out
pub const REQUEST_TIMEOUT_ERROR: &str = "Request timeout";

// =============================================================================
// Heartbeat Context for Request Timeout Extension
// =============================================================================

/// Context for request heartbeat mechanism.
/// Allows PHP scripts to extend their timeout deadline.
/// Uses Instant-based timing for minimal syscall overhead.
#[repr(C)]
pub struct HeartbeatContext {
    /// Start time (reused from queued_at for zero extra Instant::now() calls)
    start: Instant,
    /// Current deadline as milliseconds from start
    deadline_ms: AtomicU64,
    /// Maximum extension allowed per heartbeat call (= original REQUEST_TIMEOUT)
    max_extension_secs: u64,
}

impl HeartbeatContext {
    /// Creates a new heartbeat context reusing an existing Instant.
    /// This avoids calling Instant::now() which has syscall overhead.
    pub fn new(start: Instant, timeout_secs: u64) -> Self {
        let deadline_ms = timeout_secs * 1000;
        Self {
            start,
            deadline_ms: AtomicU64::new(deadline_ms),
            max_extension_secs: timeout_secs,
        }
    }

    /// Extends the deadline by `secs` seconds from now.
    /// Returns false if `secs` exceeds the max extension limit.
    pub fn heartbeat(&self, secs: u64) -> bool {
        if secs == 0 || secs > self.max_extension_secs {
            return false;
        }

        let elapsed_ms = self.start.elapsed().as_millis() as u64;
        let new_deadline_ms = elapsed_ms + secs * 1000;
        self.deadline_ms.store(new_deadline_ms, Ordering::Release);
        true
    }

    /// Returns the remaining time until deadline, or None if already expired.
    pub fn remaining(&self) -> Option<Duration> {
        let elapsed_ms = self.start.elapsed().as_millis() as u64;
        let deadline_ms = self.deadline_ms.load(Ordering::Acquire);

        if elapsed_ms >= deadline_ms {
            None
        } else {
            Some(Duration::from_millis(deadline_ms - elapsed_ms))
        }
    }

    /// Returns the max extension limit in seconds.
    pub fn max_extension(&self) -> u64 {
        self.max_extension_secs
    }
}

/// FFI callback from PHP extension to perform heartbeat.
/// Returns 1 on success, 0 on failure.
/// Takes `*mut c_void` for FFI compatibility (cast from HeartbeatContext pointer).
#[no_mangle]
pub extern "C" fn tokio_php_heartbeat(ctx: *mut std::ffi::c_void, secs: u64) -> i64 {
    if ctx.is_null() {
        return 0;
    }

    let ctx = unsafe { &*(ctx as *mut HeartbeatContext) };

    if ctx.heartbeat(secs) {
        1
    } else {
        0
    }
}

/// Generic worker pool for PHP execution
pub struct WorkerPool {
    request_tx: std_mpsc::SyncSender<WorkerRequest>,
    workers: Vec<WorkerThread>,
    worker_count: AtomicUsize,
    queue_capacity: usize,
}

impl WorkerPool {
    /// Creates a new worker pool with the given number of workers.
    /// The `worker_fn` is called for each worker thread.
    /// Queue capacity defaults to workers * 100.
    pub fn new<F>(num_workers: usize, name_prefix: &str, worker_fn: F) -> Result<Self, String>
    where
        F: Fn(usize, Arc<Mutex<std_mpsc::Receiver<WorkerRequest>>>) + Send + Clone + 'static,
    {
        Self::with_queue_capacity(
            num_workers,
            name_prefix,
            num_workers * DEFAULT_QUEUE_MULTIPLIER,
            worker_fn,
        )
    }

    /// Creates a new worker pool with custom queue capacity.
    pub fn with_queue_capacity<F>(
        num_workers: usize,
        name_prefix: &str,
        queue_capacity: usize,
        worker_fn: F,
    ) -> Result<Self, String>
    where
        F: Fn(usize, Arc<Mutex<std_mpsc::Receiver<WorkerRequest>>>) + Send + Clone + 'static,
    {
        let (request_tx, request_rx) = std_mpsc::sync_channel::<WorkerRequest>(queue_capacity);
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

        tracing::info!(
            "WorkerPool '{}' created with {} workers, queue capacity {}",
            name_prefix,
            num_workers,
            queue_capacity
        );

        Ok(Self {
            request_tx,
            workers,
            worker_count: AtomicUsize::new(num_workers),
            queue_capacity,
        })
    }

    /// Executes a request asynchronously via the worker pool.
    /// Returns QUEUE_FULL_ERROR if the queue is full.
    /// Returns REQUEST_TIMEOUT_ERROR if the request times out.
    ///
    /// Supports heartbeat mechanism: if timeout is configured, creates a HeartbeatContext
    /// that allows PHP scripts to extend the deadline via tokio_request_heartbeat().
    ///
    /// This method uses streaming internally but collects all output into a single
    /// ScriptResponse for backward compatibility. For true streaming, use
    /// `submit_streaming()` instead.
    pub async fn execute(&self, request: ScriptRequest) -> Result<ScriptResponse, String> {
        use crate::profiler::ProfileData;

        let timeout = request.timeout;

        // Capture queued_at once - reused for both queue timing and HeartbeatContext
        let queued_at = Instant::now();

        // Create heartbeat context reusing queued_at
        let heartbeat_ctx =
            timeout.map(|t| Arc::new(HeartbeatContext::new(queued_at, t.as_secs())));

        // Create streaming channel (buffer size of 32 is enough for collecting)
        let (stream_tx, mut stream_rx) = tokio_mpsc::channel::<ResponseChunk>(32);

        // Use try_send to avoid blocking and detect queue full
        self.request_tx
            .try_send(WorkerRequest {
                request,
                stream_tx,
                queued_at,
                heartbeat_ctx: heartbeat_ctx.clone(),
            })
            .map_err(|e| match e {
                std_mpsc::TrySendError::Full(_) => QUEUE_FULL_ERROR.to_string(),
                std_mpsc::TrySendError::Disconnected(_) => "Worker pool shut down".to_string(),
            })?;

        // Collect streaming response into ScriptResponse
        let mut headers: Vec<(String, String)> = Vec::new();
        let mut status: u16 = 200;
        let mut body = Vec::new();
        let mut profile: Option<ProfileData> = None;

        // Apply timeout with heartbeat support if configured
        if let Some(ctx) = heartbeat_ctx {
            loop {
                match ctx.remaining() {
                    None => {
                        return Err(REQUEST_TIMEOUT_ERROR.to_string());
                    }
                    Some(remaining) => {
                        tokio::select! {
                            biased;

                            chunk = stream_rx.recv() => {
                                match chunk {
                                    Some(ResponseChunk::Headers { status: s, headers: h }) => {
                                        status = s;
                                        headers = h;
                                    }
                                    Some(ResponseChunk::Body(data)) => {
                                        body.extend_from_slice(&data);
                                    }
                                    Some(ResponseChunk::Profile(p)) => {
                                        profile = Some(*p);
                                    }
                                    Some(ResponseChunk::End) => {
                                        break;
                                    }
                                    Some(ResponseChunk::Error(e)) => {
                                        return Err(e);
                                    }
                                    None => {
                                        return Err("Worker dropped connection".to_string());
                                    }
                                }
                            }

                            _ = tokio::time::sleep(remaining) => {
                                continue; // Check remaining() again (heartbeat may have extended)
                            }
                        }
                    }
                }
            }
        } else {
            // No timeout - just collect all chunks
            while let Some(chunk) = stream_rx.recv().await {
                match chunk {
                    ResponseChunk::Headers {
                        status: s,
                        headers: h,
                    } => {
                        status = s;
                        headers = h;
                    }
                    ResponseChunk::Body(data) => {
                        body.extend_from_slice(&data);
                    }
                    ResponseChunk::Profile(p) => {
                        profile = Some(*p);
                    }
                    ResponseChunk::End => {
                        break;
                    }
                    ResponseChunk::Error(e) => {
                        return Err(e);
                    }
                }
            }
        }

        // Add Status header if non-200
        if status != 200 {
            headers.insert(0, ("Status".to_string(), status.to_string()));
        }

        Ok(ScriptResponse {
            body: String::from_utf8_lossy(&body).into_owned(),
            headers,
            profile,
        })
    }

    /// Submits a streaming request to the worker pool.
    ///
    /// Returns immediately with a receiver for streaming response chunks.
    /// The caller should wait for `ResponseChunk::Headers` first, then stream
    /// `ResponseChunk::Body` chunks until `ResponseChunk::End`.
    ///
    /// # Arguments
    /// * `request` - The script request
    ///
    /// # Returns
    /// * `Ok(receiver)` - Receiver for response chunks
    /// * `Err(message)` - If queue is full or pool is shut down
    pub fn submit_streaming(
        &self,
        request: ScriptRequest,
    ) -> Result<tokio_mpsc::Receiver<ResponseChunk>, String> {
        let timeout = request.timeout;
        let queued_at = Instant::now();

        // Create heartbeat context
        let heartbeat_ctx =
            timeout.map(|t| Arc::new(HeartbeatContext::new(queued_at, t.as_secs())));

        // Create streaming channel with reasonable buffer
        let (stream_tx, stream_rx) = tokio_mpsc::channel::<ResponseChunk>(32);

        self.request_tx
            .try_send(WorkerRequest {
                request,
                stream_tx,
                queued_at,
                heartbeat_ctx,
            })
            .map_err(|e| match e {
                std_mpsc::TrySendError::Full(_) => QUEUE_FULL_ERROR.to_string(),
                std_mpsc::TrySendError::Disconnected(_) => "Worker pool shut down".to_string(),
            })?;

        Ok(stream_rx)
    }

    /// Legacy streaming method - delegates to submit_streaming.
    /// Deprecated: Use submit_streaming() with ResponseChunk instead.
    #[deprecated(note = "Use submit_streaming() with ResponseChunk instead")]
    #[allow(deprecated)]
    pub fn execute_streaming(
        &self,
        request: ScriptRequest,
        buffer_size: usize,
    ) -> Result<tokio_mpsc::Receiver<StreamChunk>, String> {
        // Convert new ResponseChunk stream to old StreamChunk stream
        let rx = self.submit_streaming(request)?;
        let (tx, new_rx) = tokio_mpsc::channel::<StreamChunk>(buffer_size);

        // Spawn task to convert chunks (only forward body data)
        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(chunk) = rx.recv().await {
                match chunk {
                    ResponseChunk::Body(data) => {
                        if tx.send(StreamChunk::new(data)).await.is_err() {
                            break;
                        }
                    }
                    ResponseChunk::End | ResponseChunk::Error(_) | ResponseChunk::Profile(_) => {
                        break;
                    }
                    ResponseChunk::Headers { .. } => {
                        // Headers are handled separately, skip
                    }
                }
            }
        });

        Ok(new_rx)
    }

    /// Executes a request with automatic SSE detection.
    ///
    /// Uses the new streaming infrastructure internally. If PHP sets
    /// `Content-Type: text/event-stream`, returns a streaming result.
    /// Otherwise, collects all output and returns a normal response.
    pub async fn execute_with_auto_sse(
        &self,
        request: ScriptRequest,
    ) -> Result<ExecuteResult, String> {
        use crate::profiler::ProfileData;

        let mut rx = self.submit_streaming(request)?;

        // Wait for headers chunk
        let (status, mut headers) = match rx.recv().await {
            Some(ResponseChunk::Headers { status, headers }) => (status, headers),
            Some(ResponseChunk::Error(e)) => return Err(e),
            Some(ResponseChunk::End) => {
                // Empty response (no headers sent)
                return Ok(ExecuteResult::Normal(Box::new(ScriptResponse {
                    body: String::new(),
                    headers: Vec::new(),
                    profile: None,
                })));
            }
            Some(ResponseChunk::Body(_)) => {
                // Body before headers - shouldn't happen, treat as error
                return Err("Received body chunk before headers".to_string());
            }
            Some(ResponseChunk::Profile(_)) => {
                // Profile before headers - shouldn't happen, treat as error
                return Err("Received profile chunk before headers".to_string());
            }
            None => return Err("Worker dropped connection".to_string()),
        };

        // Check if this is streaming mode:
        // 1. SSE (Content-Type: text/event-stream)
        // 2. Explicit chunked mode (x-tokio-streaming-mode: chunked from tokio_send_headers)
        let is_sse = headers.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("content-type") && v.contains("text/event-stream")
        });
        let is_chunked = headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("x-tokio-streaming-mode"));

        // Remove internal marker header before sending to client
        if is_chunked {
            headers.retain(|(k, _)| !k.eq_ignore_ascii_case("x-tokio-streaming-mode"));
        }

        if is_sse || is_chunked {
            // SSE mode: create bridge channel to convert ResponseChunk::Body -> StreamChunk
            let (tx, stream_rx) = tokio_mpsc::channel::<StreamChunk>(32);

            // Spawn task to forward body chunks
            tokio::spawn(async move {
                while let Some(chunk) = rx.recv().await {
                    match chunk {
                        ResponseChunk::Body(data) => {
                            if tx.send(StreamChunk::new(data)).await.is_err() {
                                break;
                            }
                        }
                        ResponseChunk::End
                        | ResponseChunk::Error(_)
                        | ResponseChunk::Profile(_) => {
                            break;
                        }
                        ResponseChunk::Headers { .. } => {
                            // Ignore duplicate headers
                        }
                    }
                }
            });

            Ok(ExecuteResult::Streaming {
                headers,
                status_code: status,
                receiver: stream_rx,
            })
        } else {
            // Non-SSE: collect all body chunks and profile data
            let mut body = Vec::new();
            let mut profile: Option<ProfileData> = None;

            while let Some(chunk) = rx.recv().await {
                match chunk {
                    ResponseChunk::Body(data) => {
                        body.extend_from_slice(&data);
                    }
                    ResponseChunk::Profile(p) => {
                        profile = Some(*p);
                    }
                    ResponseChunk::End => break,
                    ResponseChunk::Error(e) => return Err(e),
                    ResponseChunk::Headers { .. } => {
                        // Ignore duplicate headers
                    }
                }
            }

            // Add Status header if non-200
            let mut final_headers = headers;
            if status != 200 {
                final_headers.insert(0, ("Status".to_string(), status.to_string()));
            }

            Ok(ExecuteResult::Normal(Box::new(ScriptResponse {
                body: String::from_utf8_lossy(&body).into_owned(),
                headers: final_headers,
                profile,
            })))
        }
    }

    /// Returns the queue capacity
    pub fn queue_capacity(&self) -> usize {
        self.queue_capacity
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

/// Convert FinishData from early finish callback to ScriptResponse
#[allow(dead_code)]
fn finish_data_to_response(data: FinishData, profiling: bool) -> ScriptResponse {
    use crate::profiler::ProfileData;

    ScriptResponse {
        body: String::from_utf8_lossy(&data.body).into_owned(),
        headers: data.headers,
        profile: if profiling {
            Some(ProfileData {
                early_finish: true,
                ..Default::default()
            })
        } else {
            None
        },
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
        if i > 0 {
            code.push(',');
        }
        write_kv(&mut code, key, value);
    }
    code.push_str("];");

    // $_POST
    code.push_str("$_POST=[");
    for (i, (key, value)) in request.post_params.iter().enumerate() {
        if i > 0 {
            code.push(',');
        }
        write_kv(&mut code, key, value);
    }
    code.push_str("];");

    // $_SERVER
    code.push_str("$_SERVER=[");
    for (i, (key, value)) in request.server_vars.iter().enumerate() {
        if i > 0 {
            code.push(',');
        }
        write_kv(&mut code, key, value);
    }
    code.push_str("];");

    // $_COOKIE
    code.push_str("$_COOKIE=[");
    for (i, (key, value)) in request.cookies.iter().enumerate() {
        if i > 0 {
            code.push(',');
        }
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
            if i > 0 {
                code.push(',');
            }
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
                    if j > 0 {
                        code.push(',');
                    }
                    code.push('\'');
                    write_escaped(&mut code, &f.name);
                    code.push('\'');
                }
                code.push_str("],'type'=>[");
                for (j, f) in files_vec.iter().enumerate() {
                    if j > 0 {
                        code.push(',');
                    }
                    code.push('\'');
                    write_escaped(&mut code, &f.mime_type);
                    code.push('\'');
                }
                code.push_str("],'tmp_name'=>[");
                for (j, f) in files_vec.iter().enumerate() {
                    if j > 0 {
                        code.push(',');
                    }
                    code.push('\'');
                    write_escaped(&mut code, &f.tmp_name);
                    code.push('\'');
                }
                code.push_str("],'error'=>[");
                for (j, f) in files_vec.iter().enumerate() {
                    if j > 0 {
                        code.push(',');
                    }
                    code.push_str(&f.error.to_string());
                }
                code.push_str("],'size'=>[");
                for (j, f) in files_vec.iter().enumerate() {
                    if j > 0 {
                        code.push(',');
                    }
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
    let mut code = String::with_capacity(4096);
    code.push_str(&build_superglobals_code(request));
    code.push_str("require'");
    write_escaped(&mut code, &request.script_path);
    code.push_str("';");
    code
}

// =============================================================================
// PHP Execution
// =============================================================================

/// Timing data for profiling
#[allow(dead_code)]
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

/// Stdout capture state - keeps stdout redirected until finalized
#[allow(dead_code)]
pub struct StdoutCapture {
    write_fd: libc::c_int,
    original_stdout: libc::c_int,
}

#[allow(dead_code)]
impl StdoutCapture {
    /// Sets up stdout capture, redirecting to memfd
    pub fn new() -> Result<Self, String> {
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
            unsafe {
                libc::close(write_fd);
            }
            return Err("Failed to dup stdout".to_string());
        }

        if unsafe { libc::dup2(write_fd, 1) } < 0 {
            unsafe {
                libc::close(write_fd);
                libc::close(original_stdout);
            }
            return Err("Failed to redirect stdout".to_string());
        }

        Ok(Self {
            write_fd,
            original_stdout,
        })
    }

    /// Restores stdout and reads captured output
    pub fn finalize(self) -> String {
        unsafe {
            libc::fflush(ptr::null_mut());
            libc::dup2(self.original_stdout, 1);
            libc::close(self.original_stdout);
        }

        OUTPUT_BUFFER.with(|buf| {
            let mut buf = buf.borrow_mut();
            buf.clear();

            unsafe {
                libc::lseek(self.write_fd, 0, libc::SEEK_SET);

                let mut chunk = [0u8; 8192];
                loop {
                    let n = libc::read(
                        self.write_fd,
                        chunk.as_mut_ptr() as *mut libc::c_void,
                        chunk.len(),
                    );
                    if n <= 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n as usize]);
                }

                libc::close(self.write_fd);
            }

            String::from_utf8_lossy(&buf).into_owned()
        })
    }
}

/// Executes PHP script, returns capture handle for later finalization
/// IMPORTANT: Caller must call php_request_shutdown() before finalizing capture!
#[allow(dead_code)]
pub fn execute_php_script_start(
    request: &ScriptRequest,
    profiling: bool,
) -> Result<(StdoutCapture, ExecutionTiming), String> {
    let mut timing = ExecutionTiming::default();

    // Clear captured headers from previous request
    sapi::clear_captured_headers();

    // Build combined code
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

    // Run finalize code (flush buffers, output headers)
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

    Ok((capture, timing))
}

/// Finalizes script execution after php_request_shutdown
#[allow(dead_code)]
pub fn execute_php_script_finish(
    capture: StdoutCapture,
    mut timing: ExecutionTiming,
    profiling: bool,
    queue_wait_us: u64,
    php_startup_us: u64,
) -> Result<ScriptResponse, String> {
    // Restore stdout and read output
    let restore_start = Instant::now();
    let body = capture.finalize();
    if profiling {
        timing.stdout_restore_us = restore_start.elapsed().as_micros() as u64;
        timing.output_read_us = 0; // Included in restore
    }

    // Get headers captured via SAPI header_handler
    let parse_start = Instant::now();
    let mut headers = sapi::get_captured_headers();

    // Add Status header if http_response_code was set to non-200
    let status = sapi::get_captured_status();
    if status != 200 {
        // Insert Status at the beginning so it's processed first
        headers.insert(0, ("Status".to_string(), status.to_string()));
    }

    if profiling {
        timing.output_parse_us = parse_start.elapsed().as_micros() as u64;
    }

    let output_capture_us = timing.finalize_eval_us
        + timing.stdout_restore_us
        + timing.output_read_us
        + timing.output_parse_us;
    let superglobals_us = timing.superglobals_build_us;

    let profile = if profiling {
        Some(ProfileData {
            total_us: 0,
            queue_wait_us,
            php_startup_us,
            superglobals_us,
            superglobals_build_us: timing.superglobals_build_us,
            superglobals_eval_us: 0,
            memfd_setup_us: timing.memfd_setup_us,
            script_exec_us: timing.script_exec_us,
            output_capture_us,
            finalize_eval_us: timing.finalize_eval_us,
            stdout_restore_us: timing.stdout_restore_us,
            output_read_us: timing.output_read_us,
            output_parse_us: timing.output_parse_us,
            php_shutdown_us: 0,
            response_build_us: 0,
            ..Default::default()
        })
    } else {
        None
    };

    Ok(ScriptResponse {
        body,
        headers,
        profile,
    })
}

/// Worker thread main loop - processes requests until channel closes.
/// Uses streaming output via SAPI ub_write callback.
pub fn worker_main_loop(id: usize, rx: Arc<Mutex<std_mpsc::Receiver<WorkerRequest>>>) {
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
            Ok(WorkerRequest {
                request,
                stream_tx,
                queued_at: _,
                heartbeat_ctx: _,
            }) => {
                // Clear captured headers from previous request
                sapi::clear_captured_headers();

                // Initialize streaming state (output will go through ub_write callback)
                sapi::init_stream_state(stream_tx);

                // Start PHP request
                let startup_ok = unsafe { php_request_startup() } == 0;

                if startup_ok {
                    // Build and execute combined code (superglobals + script)
                    let combined_code = build_combined_code(&request);

                    unsafe {
                        let code_c = CString::new(combined_code).unwrap_or_default();
                        let name_c = CString::new("x").unwrap();
                        zend_eval_string(
                            code_c.as_ptr() as *mut c_char,
                            ptr::null_mut(),
                            name_c.as_ptr() as *mut c_char,
                        );

                        // Finalize code (flush PHP buffers)
                        zend_eval_string(
                            FINALIZE_CODE.as_ptr() as *mut c_char,
                            ptr::null_mut(),
                            FINALIZE_NAME.as_ptr() as *mut c_char,
                        );
                    }

                    // PHP request shutdown
                    unsafe {
                        php_request_shutdown(ptr::null_mut());
                    }
                } else {
                    // Send error if startup failed
                    sapi::send_stream_error("Failed to start PHP request".to_string());
                }

                // Finalize streaming (sends End chunk if not already sent)
                sapi::finalize_stream();
                sapi::clear_request_data();
            }
            Err(_) => {
                break;
            }
        }
    }

    tracing::debug!("Worker {}: Shutdown complete", id);
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // HeartbeatContext tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_heartbeat_context_new() {
        let start = Instant::now();
        let ctx = HeartbeatContext::new(start, 30);

        assert_eq!(ctx.max_extension(), 30);
        assert!(ctx.remaining().is_some());
    }

    #[test]
    fn test_heartbeat_context_remaining() {
        let start = Instant::now();
        let ctx = HeartbeatContext::new(start, 10);

        let remaining = ctx.remaining().unwrap();
        // Should be close to 10 seconds (allow some tolerance)
        assert!(remaining.as_secs() >= 9);
        assert!(remaining.as_secs() <= 10);
    }

    #[test]
    fn test_heartbeat_extends_deadline() {
        let start = Instant::now();
        let ctx = HeartbeatContext::new(start, 60);

        // Wait a tiny bit
        std::thread::sleep(Duration::from_millis(10));

        // Extend by 30 seconds
        assert!(ctx.heartbeat(30));

        // Check remaining is about 30 seconds from now
        let remaining = ctx.remaining().unwrap();
        assert!(remaining.as_secs() >= 29);
        assert!(remaining.as_secs() <= 30);
    }

    #[test]
    fn test_heartbeat_rejects_zero() {
        let start = Instant::now();
        let ctx = HeartbeatContext::new(start, 60);

        assert!(!ctx.heartbeat(0));
    }

    #[test]
    fn test_heartbeat_rejects_over_max() {
        let start = Instant::now();
        let ctx = HeartbeatContext::new(start, 30);

        // Try to extend by more than max (30)
        assert!(!ctx.heartbeat(31));
        assert!(!ctx.heartbeat(60));

        // But max is allowed
        assert!(ctx.heartbeat(30));
    }

    #[test]
    fn test_heartbeat_expired() {
        let start = Instant::now() - Duration::from_secs(100);
        let ctx = HeartbeatContext::new(start, 10);

        // Should be expired (started 100s ago, timeout 10s)
        assert!(ctx.remaining().is_none());
    }

    // -------------------------------------------------------------------------
    // PHP string escaping tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_needs_escape_clean_string() {
        assert!(!needs_escape("hello"));
        assert!(!needs_escape("foo bar"));
        assert!(!needs_escape("123"));
        assert!(!needs_escape(""));
    }

    #[test]
    fn test_needs_escape_with_backslash() {
        assert!(needs_escape("foo\\bar"));
        assert!(needs_escape("\\"));
    }

    #[test]
    fn test_needs_escape_with_quote() {
        assert!(needs_escape("it's"));
        assert!(needs_escape("'quoted'"));
    }

    #[test]
    fn test_needs_escape_with_null() {
        assert!(needs_escape("foo\0bar"));
    }

    #[test]
    fn test_write_escaped_clean() {
        let mut buf = String::new();
        write_escaped(&mut buf, "hello world");
        assert_eq!(buf, "hello world");
    }

    #[test]
    fn test_write_escaped_backslash() {
        let mut buf = String::new();
        write_escaped(&mut buf, "foo\\bar");
        assert_eq!(buf, "foo\\\\bar");
    }

    #[test]
    fn test_write_escaped_quote() {
        let mut buf = String::new();
        write_escaped(&mut buf, "it's");
        assert_eq!(buf, "it\\'s");
    }

    #[test]
    fn test_write_escaped_null_stripped() {
        let mut buf = String::new();
        write_escaped(&mut buf, "foo\0bar");
        assert_eq!(buf, "foobar");
    }

    #[test]
    fn test_write_escaped_mixed() {
        let mut buf = String::new();
        write_escaped(&mut buf, "path\\to\\'file'");
        assert_eq!(buf, "path\\\\to\\\\\\'file\\'");
    }

    #[test]
    fn test_write_kv() {
        let mut buf = String::new();
        write_kv(&mut buf, "key", "value");
        assert_eq!(buf, "'key'=>'value'");
    }

    #[test]
    fn test_write_kv_with_escaping() {
        let mut buf = String::new();
        write_kv(&mut buf, "it's", "O'Brien");
        assert_eq!(buf, "'it\\'s'=>'O\\'Brien'");
    }

    // -------------------------------------------------------------------------
    // PHP code generation tests
    // -------------------------------------------------------------------------

    use std::borrow::Cow;

    #[test]
    fn test_build_superglobals_code_empty() {
        let request = ScriptRequest {
            script_path: "/test.php".to_string(),
            ..Default::default()
        };

        let code = build_superglobals_code(&request);

        assert!(code.contains("$_GET=[];"));
        assert!(code.contains("$_POST=[];"));
        assert!(code.contains("$_SERVER=[];"));
        assert!(code.contains("$_COOKIE=[];"));
        assert!(code.contains("$_FILES=[];"));
        assert!(code.contains("$_REQUEST=$_GET+$_POST;"));
    }

    #[test]
    fn test_build_superglobals_code_with_get() {
        let request = ScriptRequest {
            script_path: "/test.php".to_string(),
            get_params: vec![
                (Cow::Owned("foo".to_string()), Cow::Owned("bar".to_string())),
                (Cow::Owned("num".to_string()), Cow::Owned("123".to_string())),
            ],
            ..Default::default()
        };

        let code = build_superglobals_code(&request);

        assert!(code.contains("$_GET=['foo'=>'bar','num'=>'123'];"));
    }

    #[test]
    fn test_build_superglobals_code_with_post() {
        let request = ScriptRequest {
            script_path: "/test.php".to_string(),
            post_params: vec![(
                Cow::Owned("username".to_string()),
                Cow::Owned("admin".to_string()),
            )],
            ..Default::default()
        };

        let code = build_superglobals_code(&request);

        assert!(code.contains("$_POST=['username'=>'admin'];"));
    }

    #[test]
    fn test_build_superglobals_code_escapes_values() {
        let request = ScriptRequest {
            script_path: "/test.php".to_string(),
            get_params: vec![
                (
                    Cow::Owned("path".to_string()),
                    Cow::Owned("c:\\windows".to_string()),
                ),
                (
                    Cow::Owned("name".to_string()),
                    Cow::Owned("O'Brien".to_string()),
                ),
            ],
            ..Default::default()
        };

        let code = build_superglobals_code(&request);

        assert!(code.contains("'path'=>'c:\\\\windows'"));
        assert!(code.contains("'name'=>'O\\'Brien'"));
    }

    #[test]
    fn test_build_combined_code() {
        let request = ScriptRequest {
            script_path: "/var/www/html/index.php".to_string(),
            ..Default::default()
        };

        let code = build_combined_code(&request);

        assert!(code.ends_with("require'/var/www/html/index.php';"));
    }

    #[test]
    fn test_build_combined_code_escapes_path() {
        let request = ScriptRequest {
            script_path: "/var/www/html/it's.php".to_string(),
            ..Default::default()
        };

        let code = build_combined_code(&request);

        assert!(code.ends_with("require'/var/www/html/it\\'s.php';"));
    }

    // -------------------------------------------------------------------------
    // FFI callback test
    // -------------------------------------------------------------------------

    #[test]
    fn test_tokio_php_heartbeat_null_ctx() {
        // Null context should return 0
        let result = tokio_php_heartbeat(std::ptr::null_mut(), 10);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_tokio_php_heartbeat_valid() {
        let start = Instant::now();
        let ctx = HeartbeatContext::new(start, 60);
        let ctx_ptr = &ctx as *const HeartbeatContext as *mut std::ffi::c_void;

        let result = tokio_php_heartbeat(ctx_ptr, 30);
        assert_eq!(result, 1);
    }

    #[test]
    fn test_tokio_php_heartbeat_over_max() {
        let start = Instant::now();
        let ctx = HeartbeatContext::new(start, 30);
        let ctx_ptr = &ctx as *const HeartbeatContext as *mut std::ffi::c_void;

        // Try to extend by 60s when max is 30s
        let result = tokio_php_heartbeat(ctx_ptr, 60);
        assert_eq!(result, 0);
    }
}
