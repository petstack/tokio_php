//! SAPI callback implementations.
//!
//! This module provides the callback functions that PHP calls during
//! request processing. These are `extern "C"` functions that bridge
//! PHP's C API with Rust.

use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::ptr;
use std::slice;

use bytes::Bytes;
use tokio::sync::mpsc;

use super::context::REQUEST_CTX;
use super::ffi::*;
use crate::bridge;

// ============================================================================
// Thread-Local State for Callbacks
// ============================================================================

thread_local! {
    /// Captured HTTP headers from PHP's header() calls
    pub static CAPTURED_HEADERS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };

    /// Captured HTTP status code
    pub static CAPTURED_STATUS: RefCell<u16> = const { RefCell::new(200) };

    /// Stream state for response streaming
    pub static STREAM_STATE: RefCell<Option<StreamState>> = const { RefCell::new(None) };
}

/// Stream state for sending response chunks
pub struct StreamState {
    pub tx: mpsc::Sender<ResponseChunk>,
    pub status_code: u16,
    pub headers_sent: bool,
    pub finished: bool,
}

/// Response chunk types for streaming
#[derive(Debug)]
pub enum ResponseChunk {
    Headers {
        status: u16,
        headers: Vec<(String, String)>,
    },
    Body(Bytes),
    End,
    Error(String),
    /// Profiling data (sent after End, only when profiling enabled)
    /// Boxed to reduce enum size (ProfileData is large)
    Profile(Box<crate::profiler::ProfileData>),
}

// ============================================================================
// SAPI Lifecycle Callbacks
// ============================================================================

/// Called once during SAPI startup.
pub unsafe extern "C" fn sapi_startup_callback(_module: *mut sapi_module_struct) -> c_int {
    tracing::trace!("sapi_startup_callback called");
    SUCCESS
}

/// Called once during SAPI shutdown.
pub unsafe extern "C" fn sapi_shutdown_callback(_module: *mut sapi_module_struct) -> c_int {
    tracing::trace!("sapi_shutdown_callback called");
    SUCCESS
}

/// Called at the start of each request.
pub unsafe extern "C" fn sapi_activate_callback() -> c_int {
    tracing::trace!("sapi_activate_callback called");

    // Clear per-request state
    CAPTURED_HEADERS.with(|h| h.borrow_mut().clear());
    CAPTURED_STATUS.with(|s| *s.borrow_mut() = 200);

    SUCCESS
}

/// Called at the end of each request.
pub unsafe extern "C" fn sapi_deactivate_callback() -> c_int {
    tracing::trace!("sapi_deactivate_callback called");

    // Cleanup request context
    REQUEST_CTX.with(|ctx| {
        if let Some(ref mut context) = *ctx.borrow_mut() {
            context.cleanup();
        }
    });

    SUCCESS
}

// ============================================================================
// Output Callbacks
// ============================================================================

/// Unbuffered write callback - receives all PHP output.
///
/// This is called for every piece of output PHP produces (echo, print, etc.).
pub unsafe extern "C" fn sapi_ub_write(str: *const c_char, len: usize) -> usize {
    if str.is_null() || len == 0 {
        return len;
    }

    STREAM_STATE.with(|state| {
        let mut state_ref = state.borrow_mut();
        let stream_state = match state_ref.as_mut() {
            Some(s) => s,
            None => return len, // No streaming context
        };

        // After tokio_finish_request(), discard output
        if stream_state.finished {
            return len;
        }

        // Send headers on first output
        if !stream_state.headers_sent {
            let headers = CAPTURED_HEADERS.with(|h| std::mem::take(&mut *h.borrow_mut()));
            let status = stream_state.status_code;
            let _ = stream_state
                .tx
                .blocking_send(ResponseChunk::Headers { status, headers });
            stream_state.headers_sent = true;
        }

        // Send body chunk
        let data = slice::from_raw_parts(str.cast::<u8>(), len);
        let _ = stream_state
            .tx
            .blocking_send(ResponseChunk::Body(Bytes::copy_from_slice(data)));

        len
    })
}

/// Flush callback - called by PHP's flush() function.
pub unsafe extern "C" fn sapi_flush(_server_context: *mut c_void) {
    tracing::trace!("sapi_flush called");

    // Flush PHP output buffers
    while php_output_get_level() > 0 {
        let _ = php_output_flush();
    }
}

/// Get environment variable callback.
pub unsafe extern "C" fn sapi_getenv(name: *const c_char, name_len: usize) -> *mut c_char {
    if name.is_null() || name_len == 0 {
        return ptr::null_mut();
    }

    REQUEST_CTX.with(|ctx| {
        if let Some(ref context) = *ctx.borrow() {
            let name_slice = slice::from_raw_parts(name as *const u8, name_len);
            if let Ok(name_str) = std::str::from_utf8(name_slice) {
                if let Some(value) = context.get_env(name_str) {
                    return value.as_ptr() as *mut c_char;
                }
            }
        }
        ptr::null_mut()
    })
}

// ============================================================================
// Header Callbacks
// ============================================================================

/// Header handler - captures headers set via PHP's header() function.
pub unsafe extern "C" fn sapi_header_handler(
    sapi_header: *mut sapi_header_struct,
    op: sapi_header_op_enum,
    _sapi_headers: *mut sapi_headers_struct,
) -> c_int {
    // NOTE: Do NOT read http_response_code here - it may be uninitialized garbage.
    // Status code is captured in sapi_send_headers() when PHP is ready to send.

    if sapi_header.is_null() {
        return 0;
    }

    match op {
        sapi_header_op_enum::SAPI_HEADER_REPLACE | sapi_header_op_enum::SAPI_HEADER_ADD => {
            let header_ptr = (*sapi_header).header;
            if !header_ptr.is_null() {
                if let Ok(header_str) = CStr::from_ptr(header_ptr).to_str() {
                    // Check for HTTP status line: "HTTP/1.1 500 Internal Server Error"
                    if header_str.starts_with("HTTP/") {
                        // Parse status code from "HTTP/x.x NNN reason"
                        if let Some(status_part) = header_str.split_whitespace().nth(1) {
                            if let Ok(code) = status_part.parse::<u16>() {
                                if code >= 100 && code < 600 {
                                    CAPTURED_STATUS.with(|s| *s.borrow_mut() = code);
                                    STREAM_STATE.with(|state| {
                                        if let Some(ref mut s) = *state.borrow_mut() {
                                            if !s.headers_sent {
                                                s.status_code = code;
                                            }
                                        }
                                    });
                                }
                            }
                        }
                    } else if let Some((name, value)) = header_str.split_once(':') {
                        let name = name.trim();
                        let value = value.trim();

                        CAPTURED_HEADERS.with(|h| {
                            let mut headers = h.borrow_mut();
                            if op == sapi_header_op_enum::SAPI_HEADER_REPLACE {
                                let name_lower = name.to_lowercase();
                                headers.retain(|(n, _)| n.to_lowercase() != name_lower);
                            }
                            headers.push((name.to_string(), value.to_string()));
                        });

                        // Store in bridge for PHP access
                        bridge::add_header(
                            name,
                            value,
                            op == sapi_header_op_enum::SAPI_HEADER_REPLACE,
                        );

                        // Detect SSE Content-Type for auto-streaming
                        if name.eq_ignore_ascii_case("content-type")
                            && value.to_lowercase().contains("text/event-stream")
                        {
                            bridge::try_enable_streaming();
                        }
                    }
                }
            }
        }
        sapi_header_op_enum::SAPI_HEADER_DELETE => {
            // Handle header deletion
            let header_ptr = (*sapi_header).header;
            if !header_ptr.is_null() {
                if let Ok(header_str) = CStr::from_ptr(header_ptr).to_str() {
                    let name_lower = header_str.to_lowercase();
                    CAPTURED_HEADERS.with(|h| {
                        h.borrow_mut()
                            .retain(|(n, _)| n.to_lowercase() != name_lower);
                    });
                }
            }
        }
        sapi_header_op_enum::SAPI_HEADER_DELETE_ALL => {
            CAPTURED_HEADERS.with(|h| h.borrow_mut().clear());
        }
        sapi_header_op_enum::SAPI_HEADER_SET_STATUS => {
            // http_response_code() passes the status code directly as the sapi_header pointer
            // (cast from int to void*), not as a sapi_header_struct
            let code = sapi_header as usize as u16;
            if code >= 100 && code < 600 {
                CAPTURED_STATUS.with(|s| *s.borrow_mut() = code);
                STREAM_STATE.with(|state| {
                    if let Some(ref mut s) = *state.borrow_mut() {
                        if !s.headers_sent {
                            s.status_code = code;
                        }
                    }
                });
            }
        }
        _ => {}
    }

    0 // SAPI_HEADER_ADD = 0
}

/// Send headers callback - sends all headers to client.
pub unsafe extern "C" fn sapi_send_headers(_sapi_headers: *mut sapi_headers_struct) -> c_int {
    tracing::trace!("sapi_send_headers called");

    // NOTE: We do NOT read http_response_code from sapi_headers because it's unreliable
    // under high load - the field may contain uninitialized/garbage values when PHP
    // doesn't explicitly set a status code.
    //
    // Status code is captured from:
    // 1. HTTP/1.x status line headers (e.g., "HTTP/1.1 404 Not Found")
    // 2. SAPI_HEADER_SET_STATUS operation (but this isn't called by http_response_code())
    // 3. Bridge context (set via tokio_http_response_code() PHP function)
    // 4. Default: 200
    //
    // For http_response_code() support, PHP code should use tokio_http_response_code()
    // which stores the status in the bridge context.

    // Check if status code was set via bridge
    let bridge_status = crate::bridge::get_response_code();

    STREAM_STATE.with(|state| {
        let mut state_ref = state.borrow_mut();
        if let Some(ref mut stream_state) = *state_ref {
            if !stream_state.headers_sent {
                // Use bridge status if set (non-default), otherwise use captured status
                let status = if bridge_status != 200 {
                    bridge_status
                } else {
                    stream_state.status_code
                };

                let headers = CAPTURED_HEADERS.with(|h| std::mem::take(&mut *h.borrow_mut()));
                let _ = stream_state
                    .tx
                    .blocking_send(ResponseChunk::Headers { status, headers });
                stream_state.headers_sent = true;
            }
        }
        SAPI_HEADER_SENT_SUCCESSFULLY
    })
}

// ============================================================================
// Input Callbacks
// ============================================================================

/// Read POST data for php://input.
pub unsafe extern "C" fn sapi_read_post(buffer: *mut c_char, count_bytes: usize) -> usize {
    if buffer.is_null() || count_bytes == 0 {
        return 0;
    }

    REQUEST_CTX.with(|ctx| {
        if let Some(ref mut context) = *ctx.borrow_mut() {
            let read = context.read_post(buffer, count_bytes);
            tracing::trace!(
                requested = count_bytes,
                read = read,
                "sapi_read_post called"
            );
            return read;
        }
        tracing::trace!("sapi_read_post: no context");
        0
    })
}

/// Read cookies callback.
pub unsafe extern "C" fn sapi_read_cookies() -> *mut c_char {
    REQUEST_CTX.with(|ctx| {
        if let Some(ref context) = *ctx.borrow() {
            return context.get_cookie_string();
        }
        ptr::null_mut()
    })
}

// ============================================================================
// Server Variables Callback
// ============================================================================

/// Register $_SERVER variables.
///
/// This callback is called during php_request_startup() to populate
/// the $_SERVER superglobal.
pub unsafe extern "C" fn sapi_register_server_variables(track_vars_array: *mut zval) {
    tracing::trace!("sapi_register_server_variables called");

    if track_vars_array.is_null() {
        return;
    }

    REQUEST_CTX.with(|ctx| {
        if let Some(ref context) = *ctx.borrow() {
            for (key, value) in context.server_vars() {
                if let Ok(key_c) = std::ffi::CString::new(key.as_str()) {
                    php_register_variable_safe(
                        key_c.as_ptr(),
                        value.as_ptr() as *const c_char,
                        value.len(),
                        track_vars_array,
                    );
                }
            }
        }
    });
}

// ============================================================================
// Logging Callback
// ============================================================================

/// Log message callback - receives PHP errors and messages.
pub unsafe extern "C" fn sapi_log_message(message: *const c_char, syslog_type: c_int) {
    if message.is_null() {
        return;
    }

    if let Ok(msg) = CStr::from_ptr(message).to_str() {
        let msg = msg.trim();
        if msg.is_empty() {
            return;
        }

        // Map syslog types to tracing levels
        match syslog_type {
            0..=3 => tracing::error!(target: "php", "{}", msg),
            4 => tracing::warn!(target: "php", "{}", msg),
            5 | 6 => tracing::info!(target: "php", "{}", msg),
            _ => tracing::debug!(target: "php", "{}", msg),
        }
    }
}

/// Get request time callback.
pub unsafe extern "C" fn sapi_get_request_time(request_time: *mut f64) -> zend_result {
    if !request_time.is_null() {
        *request_time = bridge::get_request_time();
    }
    SUCCESS
}

// ============================================================================
// Public API
// ============================================================================

/// Initialize stream state for a request.
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

/// Finalize stream and send End chunk.
pub fn finalize_stream() {
    STREAM_STATE.with(|state| {
        let mut state_ref = state.borrow_mut();
        if let Some(ref mut stream_state) = *state_ref {
            // Send headers if not already sent
            if !stream_state.headers_sent {
                let headers = CAPTURED_HEADERS.with(|h| std::mem::take(&mut *h.borrow_mut()));
                let _ = stream_state.tx.blocking_send(ResponseChunk::Headers {
                    status: stream_state.status_code,
                    headers,
                });
                stream_state.headers_sent = true;
            }

            // Send End if not already finished
            if !stream_state.finished {
                let _ = stream_state.tx.blocking_send(ResponseChunk::End);
            }
        }
        *state_ref = None;
    });
}

/// Mark stream as finished (for tokio_finish_request()).
///
/// Returns true if successfully marked as finished, false if already finished.
pub fn mark_stream_finished() -> bool {
    STREAM_STATE.with(|state| {
        let mut state_ref = state.borrow_mut();
        if let Some(ref mut stream_state) = *state_ref {
            if stream_state.finished {
                return false;
            }
            stream_state.finished = true;

            // Send headers if not already sent
            if !stream_state.headers_sent {
                let headers = CAPTURED_HEADERS.with(|h| std::mem::take(&mut *h.borrow_mut()));
                let _ = stream_state.tx.blocking_send(ResponseChunk::Headers {
                    status: stream_state.status_code,
                    headers,
                });
                stream_state.headers_sent = true;
            }

            // Send End
            let _ = stream_state.tx.blocking_send(ResponseChunk::End);
            return true;
        }
        false
    })
}

/// Update the status code for the current request.
pub fn set_status_code(code: u16) {
    CAPTURED_STATUS.with(|s| *s.borrow_mut() = code);
    STREAM_STATE.with(|state| {
        if let Some(ref mut s) = *state.borrow_mut() {
            if !s.headers_sent {
                s.status_code = code;
            }
        }
    });
}

/// Clear captured headers.
pub fn clear_captured_headers() {
    CAPTURED_HEADERS.with(|h| h.borrow_mut().clear());
    CAPTURED_STATUS.with(|s| *s.borrow_mut() = 200);
}

/// Get captured headers.
pub fn get_captured_headers() -> Vec<(String, String)> {
    CAPTURED_HEADERS.with(|h| h.borrow().clone())
}

/// Get captured status code.
pub fn get_captured_status() -> u16 {
    CAPTURED_STATUS.with(|s| *s.borrow())
}

/// Check if headers have been sent for the current request.
pub fn are_headers_sent() -> bool {
    STREAM_STATE.with(|state| {
        if let Some(ref s) = *state.borrow() {
            return s.headers_sent;
        }
        false
    })
}
