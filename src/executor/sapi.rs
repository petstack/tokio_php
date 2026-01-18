//! Custom SAPI initialization for PHP embed.
//!
//! This module provides PHP initialization with custom SAPI callbacks including:
//! - header_handler: capture HTTP headers set via header()
//! - register_server_variables: populate $_SERVER during php_request_startup()
//! - read_post: provide POST body for php://input
//!
//! Note: `read_cookies` callback is registered but NOT called by PHP embed SAPI.
//! Cookie data is populated via FFI in ext.rs instead.
//!
//! Uses 'cli-server' SAPI name for OPcache compatibility.
//!
//! ## Request Flow
//!
//! 1. Call `set_request_data()` with request data (for $_SERVER)
//! 2. Call `php_request_startup()` - SAPI callback populates $_SERVER
//! 3. Set $_GET, $_POST, $_COOKIE via FFI (ext.rs)
//! 4. Execute PHP script
//! 5. Call `php_request_shutdown()`
//! 6. Call `clear_request_data()`

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::Bytes;
use tokio::sync::mpsc;

// =============================================================================
// PHP FFI Bindings
// =============================================================================

/// sapi_header_struct - represents a single HTTP header
#[repr(C)]
pub struct SapiHeader {
    pub header: *mut c_char,
    pub header_len: usize,
}

/// sapi_header_op_enum - header operation type (FFI enum, variants from C side)
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)] // FFI enum - variants received from C, not constructed in Rust
pub enum SapiHeaderOp {
    Replace = 0,
    Add = 1,
    Delete = 2,
    SetStatus = 3,
}

/// sapi_headers_struct - collection of headers (matches main/SAPI.h)
/// Using byte array for zend_llist to avoid padding issues
/// zend_llist on 64-bit: 7 pointers/size_t + 1 byte + padding = 56 bytes
#[repr(C)]
pub struct SapiHeaders {
    pub headers_bytes: [u8; 56], // zend_llist as opaque bytes
    pub http_response_code: c_int,
    // ... more fields we don't need
}

/// Pointer to function types for SAPI callbacks
type StartupFn = unsafe extern "C" fn(*mut SapiModule) -> c_int;
type ShutdownFn = unsafe extern "C" fn(*mut SapiModule) -> c_int;
type ActivateFn = unsafe extern "C" fn() -> c_int;
type DeactivateFn = unsafe extern "C" fn() -> c_int;
type UbWriteFn = unsafe extern "C" fn(*const c_char, usize) -> usize;
type FlushFn = unsafe extern "C" fn(*mut c_void);
type GetStatFn = unsafe extern "C" fn() -> *mut c_void;
type GetEnvFn = unsafe extern "C" fn(*const c_char, usize) -> *mut c_char;
type ErrorFn = unsafe extern "C" fn(c_int, *const c_char, ...);
type HeaderHandlerFn =
    unsafe extern "C" fn(*mut SapiHeader, SapiHeaderOp, *mut SapiHeaders) -> c_int;
type SendHeadersFn = unsafe extern "C" fn(*mut SapiHeaders) -> c_int;
type SendHeaderFn = unsafe extern "C" fn(*mut SapiHeader, *mut c_void);
type ReadPostFn = unsafe extern "C" fn(*mut c_char, usize) -> usize;
type ReadCookiesFn = unsafe extern "C" fn() -> *mut c_char;
type RegisterVarsFn = unsafe extern "C" fn(*mut c_void);
type LogMessageFn = unsafe extern "C" fn(*const c_char, c_int);
type GetRequestTimeFn = unsafe extern "C" fn(*mut f64) -> c_int;
type TerminateProcessFn = unsafe extern "C" fn();

/// sapi_module_struct - main SAPI module structure
/// Fields must match the order in PHP source (php_embed.c)
#[repr(C)]
pub struct SapiModule {
    pub name: *mut c_char,
    pub pretty_name: *mut c_char,
    pub startup: Option<StartupFn>,
    pub shutdown: Option<ShutdownFn>,
    pub activate: Option<ActivateFn>,
    pub deactivate: Option<DeactivateFn>,
    pub ub_write: Option<UbWriteFn>,
    pub flush: Option<FlushFn>,
    pub get_stat: Option<GetStatFn>,
    pub getenv: Option<GetEnvFn>,
    pub sapi_error: Option<ErrorFn>,
    pub header_handler: Option<HeaderHandlerFn>,
    pub send_headers: Option<SendHeadersFn>,
    pub send_header: Option<SendHeaderFn>,
    pub read_post: Option<ReadPostFn>,
    pub read_cookies: Option<ReadCookiesFn>,
    pub register_server_variables: Option<RegisterVarsFn>,
    pub log_message: Option<LogMessageFn>,
    pub get_request_time: Option<GetRequestTimeFn>,
    pub terminate_process: Option<TerminateProcessFn>,
    // STANDARD_SAPI_MODULE_PROPERTIES follows - we don't modify these
    pub php_ini_path_override: *mut c_char,
    pub default_post_reader: *mut c_void,
    pub treat_data: *mut c_void,
    pub executable_location: *mut c_char,
    pub php_ini_ignore: c_int,
    pub php_ini_ignore_cwd: c_int,
    pub get_fd: *mut c_void,
    pub force_http_10: *mut c_void,
    pub get_target_uid: *mut c_void,
    pub get_target_gid: *mut c_void,
    pub input_filter: *mut c_void,
    pub ini_defaults: *mut c_void,
    pub phpinfo_as_text: c_int,
    pub ini_entries: *mut c_char,
    pub additional_functions: *mut c_void,
    pub input_filter_init: *mut c_void,
}

#[link(name = "php")]
extern "C" {
    fn php_embed_init(argc: c_int, argv: *mut *mut c_char) -> c_int;
    fn php_embed_shutdown();
    static mut php_embed_module: SapiModule;

    // Global SAPI module (copied from php_embed_module during sapi_startup)
    // This is the actual module used during request handling
    static mut sapi_module: SapiModule;

    // For registering $_SERVER variables
    fn php_register_variable_safe(
        var: *const c_char,
        val: *const c_char,
        val_len: usize,
        track_vars_array: *mut c_void,
    );
}

// Bridge library FFI - for shared header storage between Rust and PHP
// Note: These are declared in the php link block but use the tokio_bridge library
// The library is linked via build.rs when the php feature is enabled
extern "C" {
    fn tokio_bridge_add_header(
        name: *const c_char,
        name_len: usize,
        value: *const c_char,
        value_len: usize,
        replace: c_int,
    ) -> c_int;
    fn tokio_bridge_clear_headers();

    /// Try to enable streaming mode if callback is configured.
    /// Called when PHP sets Content-Type: text/event-stream header.
    /// Returns 1 if streaming was enabled, 0 if no callback configured.
    fn tokio_bridge_try_enable_streaming() -> c_int;

    /// Check if chunked transfer encoding mode is enabled.
    /// Set by PHP flush handler when flush() is called before output.
    fn tokio_bridge_is_chunked_mode() -> c_int;

    /// Mark headers as sent to client.
    /// Called after sending headers chunk.
    fn tokio_bridge_mark_headers_sent();
}

// tokio_sapi extension FFI - for SAPI flush handler
// Linked statically via build.rs
extern "C" {
    /// SAPI flush handler - sends streaming output when flush() is called in PHP.
    /// Must be registered as sapi_module.flush for standard flush() to work with SSE.
    fn tokio_sapi_flush(server_context: *mut c_void);
}

// =============================================================================
// Request Data (set before php_request_startup)
// =============================================================================

/// Request data for SAPI callbacks.
/// This must be set BEFORE calling php_request_startup().
#[derive(Default)]
pub struct RequestData<'a> {
    /// $_SERVER variables
    pub server_vars: Vec<(Cow<'a, str>, Cow<'a, str>)>,
    /// Cookies as "key1=val1; key2=val2" string (for read_cookies callback - NOT USED)
    pub cookie_string: Option<CString>,
    /// Raw POST body for php://input
    pub post_body: Option<Vec<u8>>,
    /// Current read position in post_body
    pub post_read_pos: usize,
}

// =============================================================================
// Thread-local storage
// =============================================================================

/// Owned version of RequestData for thread-local storage
struct RequestDataOwned {
    /// $_SERVER variables (owned strings)
    server_vars: Vec<(String, String)>,
    /// Cookies as "key1=val1; key2=val2" string (for read_cookies callback - NOT USED)
    cookie_string: Option<CString>,
    /// Raw POST body for php://input
    post_body: Option<Vec<u8>>,
    /// Current read position in post_body
    post_read_pos: usize,
}

/// Trace context for log correlation.
/// Stored in thread-local storage during PHP execution.
#[derive(Default, Clone)]
struct TraceContext {
    /// Unique request identifier (e.g., "65bdbab40000")
    request_id: String,
    /// W3C trace ID (32 hex chars)
    trace_id: String,
    /// W3C span ID (16 hex chars)
    span_id: String,
}

thread_local! {
    /// Captured headers for current request
    pub static CAPTURED_HEADERS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    /// Captured HTTP status code
    pub static CAPTURED_STATUS: RefCell<u16> = const { RefCell::new(200) };
    /// Request data for SAPI callbacks (set before php_request_startup)
    static REQUEST_DATA: RefCell<Option<RequestDataOwned>> = const { RefCell::new(None) };
    /// Streaming state for current request (set when streaming is enabled)
    static STREAM_STATE: RefCell<Option<StreamState>> = const { RefCell::new(None) };
    /// Trace context for log correlation (set before PHP execution)
    static TRACE_CTX: RefCell<TraceContext> = const { RefCell::new(TraceContext {
        request_id: String::new(),
        trace_id: String::new(),
        span_id: String::new(),
    }) };
    /// Virtual environment variables for getenv() (cleared per request)
    /// Maps env var name -> cached CString for FFI
    static VIRTUAL_ENV: RefCell<HashMap<String, CString>> = RefCell::new(HashMap::new());
    /// Temporary files to clean up after request (e.g., $_FILES uploads)
    static TEMP_FILES: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

// =============================================================================
// HTTP Streaming Support
// =============================================================================

/// Response chunk types for streaming HTTP responses.
/// Sent through mpsc channel from worker to HTTP handler.
#[derive(Debug)]
pub enum ResponseChunk {
    /// HTTP headers (sent once, must be first chunk)
    Headers {
        status: u16,
        headers: Vec<(String, String)>,
    },
    /// Body data chunk
    Body(Bytes),
    /// End of response (script finished or tokio_finish_request called)
    End,
    /// Error occurred during execution
    Error(String),
    /// Profiling data (sent after End, only when profiling enabled)
    /// Boxed to reduce enum size (ProfileData is large)
    Profile(Box<crate::profiler::ProfileData>),
}

/// Streaming state for current request.
/// Stored in thread-local storage during PHP execution.
struct StreamState {
    /// Channel sender for response chunks
    tx: mpsc::Sender<ResponseChunk>,
    /// HTTP status code (can be changed via header() before output)
    status_code: u16,
    /// Whether headers have been sent to the client
    headers_sent: bool,
    /// Whether tokio_finish_request() was called
    finished: bool,
}

/// SAPI ub_write callback - called for each output from PHP.
/// This is called AFTER PHP's output buffering (ob_*), so we receive
/// data when PHP decides to actually output it (buffer full, flush(), script end).
///
/// # Safety
/// Called from PHP via FFI. The str pointer is valid for the duration of the call.
unsafe extern "C" fn stream_ub_write(str: *const c_char, len: usize) -> usize {
    // Quick path: if no streaming state, this is likely during PHP startup/shutdown
    // Return len to indicate all bytes were "written"
    STREAM_STATE.with(|state| {
        let mut state_ref = state.borrow_mut();
        let stream_state = match state_ref.as_mut() {
            Some(s) => s,
            None => return len, // No streaming context - ignore output
        };

        // After tokio_finish_request(), output is discarded
        if stream_state.finished {
            return len;
        }

        // First output triggers header sending
        if !stream_state.headers_sent {
            // Take headers from CAPTURED_HEADERS (populated by header_handler)
            let headers = CAPTURED_HEADERS.with(|h| std::mem::take(&mut *h.borrow_mut()));
            // Filter headers for streaming (remove Content-Length if chunked mode)
            let headers = filter_headers_for_streaming(headers);
            let status = stream_state.status_code;

            // Send headers chunk (blocking_send is ok - we're in a worker thread)
            let _ = stream_state
                .tx
                .blocking_send(ResponseChunk::Headers { status, headers });
            stream_state.headers_sent = true;
            // Mark headers as sent in bridge TLS
            tokio_bridge_mark_headers_sent();
        }

        // Send body chunk
        if len > 0 {
            let data = std::slice::from_raw_parts(str.cast::<u8>(), len);
            let _ = stream_state
                .tx
                .blocking_send(ResponseChunk::Body(Bytes::copy_from_slice(data)));
        }

        len
    })
}

/// Initialize streaming state for current request.
/// Must be called BEFORE PHP script execution starts.
///
/// # Arguments
/// * `tx` - Channel sender for response chunks
pub fn init_stream_state(tx: mpsc::Sender<ResponseChunk>) {
    STREAM_STATE.with(|state| {
        *state.borrow_mut() = Some(StreamState {
            tx,
            status_code: 200,
            headers_sent: false,
            finished: false,
        });
    });
}

/// Internal header name used to signal chunked streaming mode.
/// This is filtered out before sending to the client.
const CHUNKED_MODE_HEADER: &str = "x-tokio-streaming-mode";

/// Filter headers for streaming: remove Content-Length when in chunked mode.
/// Checks the bridge's chunked_mode flag (set by PHP flush handler or tokio_send_headers).
/// Also adds an internal marker header to signal the executor to use streaming mode.
fn filter_headers_for_streaming(mut headers: Vec<(String, String)>) -> Vec<(String, String)> {
    // Check if chunked mode is enabled via bridge (set by tokio_send_headers or flush)
    let chunked = unsafe { tokio_bridge_is_chunked_mode() != 0 };
    if !chunked {
        return headers;
    }
    // Remove Content-Length and add streaming marker
    headers.retain(|(name, _)| !name.eq_ignore_ascii_case("content-length"));
    headers.push((CHUNKED_MODE_HEADER.to_string(), "chunked".to_string()));
    headers
}

/// Finalize streaming for current request.
/// Called after PHP script execution completes (including php_request_shutdown).
/// Sends End chunk if not already finished, and cleans up state.
pub fn finalize_stream() {
    STREAM_STATE.with(|state| {
        let mut state_ref = state.borrow_mut();
        if let Some(stream_state) = state_ref.as_mut() {
            // If no output occurred, send headers now (empty response)
            if !stream_state.headers_sent {
                let headers = CAPTURED_HEADERS.with(|h| std::mem::take(&mut *h.borrow_mut()));
                // Filter headers for streaming (remove Content-Length if chunked mode)
                let headers = filter_headers_for_streaming(headers);
                let status = stream_state.status_code;
                let _ = stream_state
                    .tx
                    .blocking_send(ResponseChunk::Headers { status, headers });
                stream_state.headers_sent = true;
                // Mark headers as sent in bridge TLS
                unsafe {
                    tokio_bridge_mark_headers_sent();
                }
            }

            // Send End chunk (unless already finished via tokio_finish_request)
            if !stream_state.finished {
                let _ = stream_state.tx.blocking_send(ResponseChunk::End);
            }
        }

        // Clean up state
        *state_ref = None;
    });
}

/// Mark response as finished (called from tokio_finish_request).
/// Sends End chunk immediately, subsequent output is discarded.
/// Returns true if this was the first finish call, false if already finished.
pub fn mark_stream_finished() -> bool {
    STREAM_STATE.with(|state| {
        let mut state_ref = state.borrow_mut();
        if let Some(stream_state) = state_ref.as_mut() {
            if stream_state.finished {
                return false; // Already finished
            }

            stream_state.finished = true;

            // If no output yet, send headers first
            if !stream_state.headers_sent {
                let headers = CAPTURED_HEADERS.with(|h| std::mem::take(&mut *h.borrow_mut()));
                // Filter headers for streaming (remove Content-Length if chunked mode)
                let headers = filter_headers_for_streaming(headers);
                let status = stream_state.status_code;
                let _ = stream_state
                    .tx
                    .blocking_send(ResponseChunk::Headers { status, headers });
                stream_state.headers_sent = true;
                // Mark headers as sent in bridge TLS
                unsafe {
                    tokio_bridge_mark_headers_sent();
                }
            }

            // Send End chunk - client receives response now
            let _ = stream_state.tx.blocking_send(ResponseChunk::End);
            return true;
        }
        false
    })
}

/// Send error chunk through streaming channel.
/// Used when PHP execution fails.
pub fn send_stream_error(error: String) {
    STREAM_STATE.with(|state| {
        let state_ref = state.borrow();
        if let Some(stream_state) = state_ref.as_ref() {
            if !stream_state.finished {
                let _ = stream_state.tx.blocking_send(ResponseChunk::Error(error));
            }
        }
    });
}

/// Get a clone of the stream sender for sending profile data.
/// Must be called BEFORE finalize_stream() as that clears the state.
/// Returns None if no streaming state is active.
pub fn get_stream_sender() -> Option<mpsc::Sender<ResponseChunk>> {
    STREAM_STATE.with(|state| state.borrow().as_ref().map(|s| s.tx.clone()))
}

/// Check if streaming is active for current request.
pub fn is_streaming_active() -> bool {
    STREAM_STATE.with(|state| state.borrow().is_some())
}

/// Check if headers have been sent for current streaming request.
pub fn are_headers_sent() -> bool {
    STREAM_STATE.with(|state| {
        state
            .borrow()
            .as_ref()
            .map(|s| s.headers_sent)
            .unwrap_or(false)
    })
}

/// Update HTTP status code for streaming response.
/// Only effective before headers are sent.
pub fn set_stream_status(status: u16) {
    STREAM_STATE.with(|state| {
        let mut state_ref = state.borrow_mut();
        if let Some(stream_state) = state_ref.as_mut() {
            if !stream_state.headers_sent {
                stream_state.status_code = status;
            }
        }
    });
}

/// Custom header handler - captures headers set via header()
/// Stores headers in both Rust's CAPTURED_HEADERS and the bridge TLS (for PHP access).
/// In streaming mode, rejects headers after output has started (PHP will emit warning).
unsafe extern "C" fn custom_header_handler(
    sapi_header: *mut SapiHeader,
    op: SapiHeaderOp,
    sapi_headers: *mut SapiHeaders,
) -> c_int {
    // Check if headers already sent in streaming mode
    // PHP will emit "headers already sent" warning, we just need to ignore the header
    let headers_already_sent = STREAM_STATE.with(|state| {
        state
            .borrow()
            .as_ref()
            .map(|s| s.headers_sent)
            .unwrap_or(false)
    });

    if headers_already_sent {
        // Return success but don't store - PHP handles the warning
        return 0;
    }

    // Always check http_response_code from sapi_headers (set by header() third arg)
    if !sapi_headers.is_null() {
        let code = (*sapi_headers).http_response_code as u16;
        if code > 0 && code != 200 {
            CAPTURED_STATUS.with(|s| {
                *s.borrow_mut() = code;
            });
            // Also update streaming state status
            set_stream_status(code);
        }
    }

    if sapi_header.is_null() {
        return 0; // SAPI_HEADER_ADD
    }

    match op {
        SapiHeaderOp::Replace | SapiHeaderOp::Add => {
            let header_ptr = (*sapi_header).header;
            if !header_ptr.is_null() {
                if let Ok(header_str) = CStr::from_ptr(header_ptr).to_str() {
                    if let Some((name, value)) = header_str.split_once(':') {
                        let name = name.trim();
                        let value = value.trim();
                        let replace = if op == SapiHeaderOp::Replace { 1 } else { 0 };

                        // Store in Rust's thread-local (for normal request completion)
                        CAPTURED_HEADERS.with(|h| {
                            let mut headers = h.borrow_mut();
                            if op == SapiHeaderOp::Replace {
                                // Remove existing headers with same name (case-insensitive)
                                let name_lower = name.to_lowercase();
                                headers.retain(|(n, _)| n.to_lowercase() != name_lower);
                            }
                            headers.push((name.to_string(), value.to_string()));
                        });

                        // Also store in bridge TLS (for PHP finish_request access)
                        tokio_bridge_add_header(
                            name.as_ptr() as *const c_char,
                            name.len(),
                            value.as_ptr() as *const c_char,
                            value.len(),
                            replace,
                        );

                        // Detect Content-Type: text/event-stream and enable streaming
                        // This allows SSE without requiring Accept header from client
                        if name.eq_ignore_ascii_case("content-type")
                            && value.to_lowercase().contains("text/event-stream")
                        {
                            tokio_bridge_try_enable_streaming();
                        }
                    }
                }
            }
        }
        SapiHeaderOp::Delete => {
            let header_ptr = (*sapi_header).header;
            if !header_ptr.is_null() {
                if let Ok(header_str) = CStr::from_ptr(header_ptr).to_str() {
                    let name_lower = header_str.trim().to_lowercase();
                    CAPTURED_HEADERS.with(|h| {
                        h.borrow_mut()
                            .retain(|(n, _)| n.to_lowercase() != name_lower);
                    });
                    // Note: bridge doesn't support delete yet, but headers are cleared per request
                }
            }
        }
        SapiHeaderOp::SetStatus => {
            // Status already handled above
        }
    }

    0 // SAPI_HEADER_ADD - let PHP also handle it
}

// =============================================================================
// SAPI Callbacks for superglobals (called during php_request_startup)
// =============================================================================

/// SAPI callback: register $_SERVER variables
/// Called by PHP during php_request_startup() -> php_hash_environment()
unsafe extern "C" fn custom_register_server_variables(track_vars_array: *mut c_void) {
    REQUEST_DATA.with(|data| {
        let data = data.borrow();
        if let Some(ref req) = *data {
            for (key, value) in &req.server_vars {
                // Create null-terminated key
                let key_cstr = match CString::new(key.as_str()) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                // php_register_variable_safe handles the value (doesn't need null terminator)
                php_register_variable_safe(
                    key_cstr.as_ptr(),
                    value.as_ptr() as *const c_char,
                    value.len(),
                    track_vars_array,
                );
            }
        }
    });
}

/// SAPI callback: read cookies
/// Called by PHP during sapi_activate() to get cookie string
/// Note: This callback is NOT called by PHP embed SAPI - using FFI instead
/// Returns pointer to "key1=val1; key2=val2" string (PHP copies it)
unsafe extern "C" fn custom_read_cookies() -> *mut c_char {
    REQUEST_DATA.with(|data| {
        let data = data.borrow();
        if let Some(ref req) = *data {
            if let Some(ref cookie_str) = req.cookie_string {
                return cookie_str.as_ptr() as *mut c_char;
            }
        }
        ptr::null_mut()
    })
}

/// SAPI callback: read POST body
/// Called by PHP to read php://input data
/// Returns number of bytes read into buffer
unsafe extern "C" fn custom_read_post(buffer: *mut c_char, count_bytes: usize) -> usize {
    REQUEST_DATA.with(|data| {
        let mut data = data.borrow_mut();
        if let Some(ref mut req) = *data {
            if let Some(ref body) = req.post_body {
                let remaining = body.len().saturating_sub(req.post_read_pos);
                let to_read = remaining.min(count_bytes);

                if to_read > 0 {
                    ptr::copy_nonoverlapping(
                        body.as_ptr().add(req.post_read_pos),
                        buffer.cast::<u8>(),
                        to_read,
                    );
                    req.post_read_pos += to_read;
                }
                return to_read;
            }
        }
        0
    })
}

/// SAPI callback: get request time
/// Called by PHP for $_SERVER['REQUEST_TIME'] and $_SERVER['REQUEST_TIME_FLOAT'].
/// Returns the Unix timestamp when the HTTP request was received by tokio_php.
/// Returns 0 (SUCCESS) always.
unsafe extern "C" fn custom_get_request_time(request_time: *mut f64) -> c_int {
    if !request_time.is_null() {
        *request_time = crate::bridge::get_request_time();
    }
    0 // SUCCESS
}

/// SAPI callback: log message
/// Called by PHP when error_log() is invoked or when PHP logs errors/warnings.
/// Routes messages to our structured JSON logger with trace correlation.
///
/// # Arguments
/// * `message` - The log message (null-terminated C string)
/// * `syslog_type` - PHP's internal log type (LOG_* constants from syslog.h)
///
/// # Syslog levels (from syslog.h):
/// - LOG_EMERG (0), LOG_ALERT (1), LOG_CRIT (2) -> ERROR
/// - LOG_ERR (3) -> ERROR
/// - LOG_WARNING (4) -> WARN
/// - LOG_NOTICE (5), LOG_INFO (6) -> INFO
/// - LOG_DEBUG (7) -> DEBUG
unsafe extern "C" fn custom_log_message(message: *const c_char, syslog_type: c_int) {
    if message.is_null() {
        return;
    }

    // Parse message
    let msg = match CStr::from_ptr(message).to_str() {
        Ok(s) => s.trim(),
        Err(_) => return,
    };

    // Skip empty messages
    if msg.is_empty() {
        return;
    }

    // Get trace context
    let (request_id, trace_id, span_id) = TRACE_CTX.with(|ctx| {
        let ctx = ctx.borrow();
        (
            ctx.request_id.clone(),
            ctx.trace_id.clone(),
            ctx.span_id.clone(),
        )
    });

    // Map syslog level to tracing level and log
    // Note: we use explicit match to avoid the overhead of creating spans
    match syslog_type {
        0..=3 => {
            // LOG_EMERG, LOG_ALERT, LOG_CRIT, LOG_ERR -> ERROR
            tracing::error!(
                target: "php",
                request_id = %request_id,
                trace_id = %trace_id,
                span_id = %span_id,
                "{}",
                msg
            );
        }
        4 => {
            // LOG_WARNING -> WARN
            tracing::warn!(
                target: "php",
                request_id = %request_id,
                trace_id = %trace_id,
                span_id = %span_id,
                "{}",
                msg
            );
        }
        5 | 6 => {
            // LOG_NOTICE, LOG_INFO -> INFO
            tracing::info!(
                target: "php",
                request_id = %request_id,
                trace_id = %trace_id,
                span_id = %span_id,
                "{}",
                msg
            );
        }
        _ => {
            // LOG_DEBUG and unknown -> DEBUG
            tracing::debug!(
                target: "php",
                request_id = %request_id,
                trace_id = %trace_id,
                span_id = %span_id,
                "{}",
                msg
            );
        }
    }
}

/// SAPI callback: get environment variable
/// Called by PHP's getenv() function.
/// First checks virtual environment variables, then falls back to real getenv.
///
/// # Arguments
/// * `name` - Environment variable name (NOT null-terminated, length provided)
/// * `name_len` - Length of the name string
///
/// # Returns
/// * Pointer to null-terminated string value (from our thread-local storage)
/// * null if not found in virtual env (PHP will fall back to real getenv)
///
/// # Safety
/// Called from PHP via FFI. The returned pointer must remain valid until
/// the next request or clear_virtual_env() call.
unsafe extern "C" fn custom_getenv(name: *const c_char, name_len: usize) -> *mut c_char {
    if name.is_null() || name_len == 0 {
        return ptr::null_mut();
    }

    // Convert name to Rust string
    let name_slice = std::slice::from_raw_parts(name as *const u8, name_len);
    let name_str = match std::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };

    // Check virtual environment variables
    VIRTUAL_ENV.with(|env| {
        let env = env.borrow();
        if let Some(cstring) = env.get(name_str) {
            // Return pointer to our cached CString (valid until clear_virtual_env)
            cstring.as_ptr() as *mut c_char
        } else {
            // Not found - return null so PHP falls back to real getenv
            ptr::null_mut()
        }
    })
}

/// SAPI callback: activate (request initialization)
/// Called by PHP after php_request_startup() completes.
/// Used to initialize per-request resources.
///
/// # Returns
/// * 0 on success (SUCCESS)
/// * -1 on failure (FAILURE)
unsafe extern "C" fn custom_activate() -> c_int {
    // Clear virtual environment variables from previous request
    // (This is a safety net - ext.rs should call clear_virtual_env() explicitly)
    VIRTUAL_ENV.with(|env| env.borrow_mut().clear());

    // Clear temp files list (files themselves are cleaned in deactivate)
    TEMP_FILES.with(|files| files.borrow_mut().clear());

    0 // SUCCESS
}

/// SAPI callback: deactivate (request cleanup)
/// Called by PHP before php_request_shutdown() completes.
/// Used to clean up per-request resources.
///
/// # Returns
/// * 0 on success (SUCCESS)
/// * -1 on failure (FAILURE)
unsafe extern "C" fn custom_deactivate() -> c_int {
    // Clean up temporary files (e.g., uploaded files from $_FILES)
    cleanup_temp_files();

    // Clear trace context (safety net - ext.rs should also call clear_trace_context)
    TRACE_CTX.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        ctx.request_id.clear();
        ctx.trace_id.clear();
        ctx.span_id.clear();
    });

    0 // SUCCESS
}

/// Clean up temporary files registered during request processing.
/// Called during deactivate phase.
fn cleanup_temp_files() {
    TEMP_FILES.with(|files| {
        let files_list = files.borrow();
        for path in files_list.iter() {
            if let Err(e) = std::fs::remove_file(path) {
                // Log but don't fail - file might already be deleted
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "Failed to clean up temp file"
                    );
                }
            }
        }
    });
    // Clear the list after cleanup
    TEMP_FILES.with(|files| files.borrow_mut().clear());
}

/// SAPI send_headers return codes (from PHP SAPI.h)
const SAPI_HEADER_SENT_SUCCESSFULLY: c_int = 1;
#[allow(dead_code)]
const SAPI_HEADER_DO_SEND: c_int = 2;
#[allow(dead_code)]
const SAPI_HEADER_SEND_FAILED: c_int = 0;

/// SAPI callback: send_headers
/// Called by PHP when it's ready to send HTTP headers to the client.
/// This happens before the first output or when headers are explicitly flushed.
///
/// For streaming responses, this allows headers to be sent immediately,
/// improving time-to-first-byte for SSE and other streaming scenarios.
///
/// # Arguments
/// * `sapi_headers` - Pointer to sapi_headers_struct containing HTTP response code
///
/// # Returns
/// * SAPI_HEADER_SENT_SUCCESSFULLY (1) on success
/// * SAPI_HEADER_SEND_FAILED (0) on failure
unsafe extern "C" fn custom_send_headers(sapi_headers: *mut SapiHeaders) -> c_int {
    STREAM_STATE.with(|state| {
        let mut state_ref = state.borrow_mut();
        let stream_state = match state_ref.as_mut() {
            Some(s) => s,
            None => return SAPI_HEADER_SENT_SUCCESSFULLY, // No streaming context
        };

        // If headers already sent, nothing to do
        if stream_state.headers_sent {
            return SAPI_HEADER_SENT_SUCCESSFULLY;
        }

        // Get HTTP status code from sapi_headers struct
        let status = if !sapi_headers.is_null() {
            let code = (*sapi_headers).http_response_code as u16;
            if code > 0 {
                code
            } else {
                stream_state.status_code
            }
        } else {
            stream_state.status_code
        };

        // Take headers from CAPTURED_HEADERS (populated by header_handler)
        let headers = CAPTURED_HEADERS.with(|h| std::mem::take(&mut *h.borrow_mut()));
        // Filter headers for streaming (remove Content-Length if chunked mode)
        let headers = filter_headers_for_streaming(headers);

        // Send headers chunk immediately
        let _ = stream_state
            .tx
            .blocking_send(ResponseChunk::Headers { status, headers });
        stream_state.headers_sent = true;
        // Mark headers as sent in bridge TLS
        tokio_bridge_mark_headers_sent();

        SAPI_HEADER_SENT_SUCCESSFULLY
    })
}

// =============================================================================
// SAPI Configuration
// =============================================================================

static SAPI_NAME: &[u8] = b"cli-server\0";
static SAPI_INITIALIZED: AtomicBool = AtomicBool::new(false);

// =============================================================================
// Public API
// =============================================================================

/// Initialize PHP with custom SAPI settings (call once at startup)
pub fn init() -> Result<(), String> {
    if SAPI_INITIALIZED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    tracing::info!("sapi::init() - initializing PHP with custom SAPI callbacks");

    unsafe {
        // Set SAPI name for OPcache compatibility
        php_embed_module.name = SAPI_NAME.as_ptr() as *mut c_char;

        // Install custom callbacks BEFORE php_embed_init
        // (these get copied to sapi_module during sapi_startup)
        php_embed_module.header_handler = Some(custom_header_handler);
        php_embed_module.register_server_variables = Some(custom_register_server_variables);
        php_embed_module.read_cookies = Some(custom_read_cookies);
        php_embed_module.read_post = Some(custom_read_post);
        php_embed_module.get_request_time = Some(custom_get_request_time); // REQUEST_TIME_FLOAT
        php_embed_module.log_message = Some(custom_log_message); // Structured logging
        php_embed_module.getenv = Some(custom_getenv); // Virtual environment variables
        php_embed_module.activate = Some(custom_activate); // Request initialization
        php_embed_module.deactivate = Some(custom_deactivate); // Request cleanup
        php_embed_module.send_headers = Some(custom_send_headers); // Early header sending
        php_embed_module.flush = Some(tokio_sapi_flush); // SSE streaming support
        php_embed_module.ub_write = Some(stream_ub_write); // HTTP streaming output

        let program_name = CString::new("tokio_php").unwrap();
        let mut argv: [*mut c_char; 2] = [program_name.as_ptr() as *mut c_char, ptr::null_mut()];

        if php_embed_init(1, argv.as_mut_ptr()) != 0 {
            return Err("Failed to initialize PHP embed".to_string());
        }

        // Also patch sapi_module directly (the global that PHP actually uses)
        // This is needed because sapi_startup() copies php_embed_module to sapi_module
        sapi_module.name = SAPI_NAME.as_ptr() as *mut c_char;
        sapi_module.header_handler = Some(custom_header_handler);
        sapi_module.register_server_variables = Some(custom_register_server_variables);
        sapi_module.read_cookies = Some(custom_read_cookies);
        sapi_module.read_post = Some(custom_read_post);
        sapi_module.get_request_time = Some(custom_get_request_time); // REQUEST_TIME_FLOAT
        sapi_module.log_message = Some(custom_log_message); // Structured logging
        sapi_module.getenv = Some(custom_getenv); // Virtual environment variables
        sapi_module.activate = Some(custom_activate); // Request initialization
        sapi_module.deactivate = Some(custom_deactivate); // Request cleanup
        sapi_module.send_headers = Some(custom_send_headers); // Early header sending
        sapi_module.flush = Some(tokio_sapi_flush); // SSE streaming support
        sapi_module.ub_write = Some(stream_ub_write); // HTTP streaming output
    }

    tracing::info!(
        "PHP initialized with SAPI 'cli-server' (ub_write, header_handler, flush, register_server_variables, get_request_time, log_message, getenv, activate, deactivate, send_headers)"
    );
    Ok(())
}

/// Shutdown PHP
pub fn shutdown() {
    if !SAPI_INITIALIZED.swap(false, Ordering::SeqCst) {
        return;
    }

    unsafe {
        php_embed_shutdown();
    }

    tracing::info!("PHP shutdown complete");
}

/// Clear captured headers (call before each request)
pub fn clear_captured_headers() {
    CAPTURED_HEADERS.with(|h| h.borrow_mut().clear());
    CAPTURED_STATUS.with(|s| *s.borrow_mut() = 200);
    // Also clear bridge TLS headers
    unsafe {
        tokio_bridge_clear_headers();
    }
}

/// Get captured headers (call after request execution)
pub fn get_captured_headers() -> Vec<(String, String)> {
    CAPTURED_HEADERS.with(|h| h.borrow().clone())
}

/// Get captured HTTP status code
pub fn get_captured_status() -> u16 {
    CAPTURED_STATUS.with(|s| *s.borrow())
}

/// Set request data for SAPI callbacks.
/// MUST be called BEFORE php_request_startup() for superglobals to be populated correctly.
///
/// # Arguments
/// * `server_vars` - $_SERVER variables (populated via register_server_variables callback)
/// * `cookies` - Cookie key-value pairs (NOT USED - read_cookies callback not called by embed SAPI)
/// * `post_body` - Raw POST body for php://input
pub fn set_request_data(
    server_vars: &[(Cow<'_, str>, Cow<'_, str>)],
    cookies: &[(Cow<'_, str>, Cow<'_, str>)],
    post_body: Option<&[u8]>,
) {
    // Format cookies as "key1=val1; key2=val2" (kept for potential future use)
    let cookie_string = if cookies.is_empty() {
        None
    } else {
        let cookie_str: String = cookies
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("; ");
        CString::new(cookie_str).ok()
    };

    REQUEST_DATA.with(|data| {
        *data.borrow_mut() = Some(RequestDataOwned {
            server_vars: server_vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            cookie_string,
            post_body: post_body.map(|b| b.to_vec()),
            post_read_pos: 0,
        });
    });
}

/// Clear request data after php_request_shutdown().
/// This frees the thread-local request data.
pub fn clear_request_data() {
    REQUEST_DATA.with(|data| {
        *data.borrow_mut() = None;
    });
}

/// Set trace context for log correlation.
/// Must be called before PHP execution to enable trace correlation in logs.
///
/// # Arguments
/// * `request_id` - Unique request identifier (e.g., "65bdbab40000")
/// * `trace_id` - W3C trace ID (32 hex chars)
/// * `span_id` - W3C span ID (16 hex chars)
pub fn set_trace_context(request_id: &str, trace_id: &str, span_id: &str) {
    TRACE_CTX.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        ctx.request_id = request_id.to_string();
        ctx.trace_id = trace_id.to_string();
        ctx.span_id = span_id.to_string();
    });
}

/// Clear trace context after PHP execution.
/// This resets the trace context for the next request.
pub fn clear_trace_context() {
    TRACE_CTX.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        ctx.request_id.clear();
        ctx.trace_id.clear();
        ctx.span_id.clear();
    });
}

/// Set a virtual environment variable.
/// These are accessible via PHP's getenv() without polluting the real process environment.
///
/// # Arguments
/// * `name` - Environment variable name (e.g., "TOKIO_REQUEST_ID")
/// * `value` - Environment variable value
///
/// # Panics
/// Panics if the value contains a null byte (invalid for C strings).
pub fn set_virtual_env(name: &str, value: &str) {
    let cstring = CString::new(value).expect("virtual env value contains null byte");
    VIRTUAL_ENV.with(|env| {
        env.borrow_mut().insert(name.to_string(), cstring);
    });
}

/// Clear all virtual environment variables.
/// Should be called after PHP execution to prevent leaking between requests.
pub fn clear_virtual_env() {
    VIRTUAL_ENV.with(|env| {
        env.borrow_mut().clear();
    });
}

/// Register a temporary file for cleanup after request processing.
/// Files registered here will be automatically deleted during the SAPI deactivate phase.
///
/// # Arguments
/// * `path` - Path to the temporary file (e.g., uploaded file from $_FILES)
///
/// # Example
/// ```ignore
/// // In file upload handling code:
/// sapi::register_temp_file(PathBuf::from("/tmp/phpXXXXXX"));
/// ```
pub fn register_temp_file(path: PathBuf) {
    TEMP_FILES.with(|files| {
        files.borrow_mut().push(path);
    });
}

/// Get the count of registered temporary files.
/// Useful for testing and debugging.
pub fn temp_file_count() -> usize {
    TEMP_FILES.with(|files| files.borrow().len())
}
