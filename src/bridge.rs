//! Rust bindings for tokio_bridge shared library.
//!
//! This module provides FFI bindings to the shared library that enables
//! direct communication between Rust and PHP, solving the TLS isolation
//! problem between statically linked Rust code and dynamically loaded PHP extensions.
//!
//! # Features
//!
//! - Shared TLS context accessible from both Rust and PHP
//! - Finish request state (fastcgi_finish_request analog)
//! - Heartbeat for request timeout extension
//! - Streaming support for SSE (Server-Sent Events)
//!
//! # Usage
//!
//! ```rust,ignore
//! use tokio_php::bridge;
//!
//! // Initialize context for a request
//! bridge::init_ctx(request_id, worker_id);
//!
//! // Set callbacks before PHP execution (unsafe - raw pointers)
//! unsafe {
//!     bridge::set_heartbeat(heartbeat_ctx, max_secs, heartbeat_callback);
//! }
//!
//! // ... execute PHP ...
//!
//! // Check if finish_request was called
//! if bridge::is_finished() {
//!     let offset = bridge::get_finished_offset();
//!     let status = bridge::get_finished_response_code();
//!     // Truncate output to offset
//! }
//!
//! // Cleanup
//! bridge::destroy_ctx();
//! ```

use std::ffi::{c_char, c_int, c_void};
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::server::response::StreamChunk;

// =============================================================================
// FFI Bindings
// =============================================================================

/// Callback type for heartbeat (request timeout extension).
pub type HeartbeatCallback = extern "C" fn(ctx: *mut c_void, secs: u64) -> i64;

/// Callback type for finish request signal (streaming response).
///
/// Called when PHP invokes `tokio_finish_request()` to send response immediately.
pub type FinishCallback = extern "C" fn(
    ctx: *mut c_void,
    body: *const c_char,
    body_len: usize,
    headers: *const c_char,
    headers_len: usize,
    header_count: c_int,
    status_code: c_int,
);

/// Callback type for streaming chunks (SSE support).
///
/// Called when PHP flushes output in streaming mode.
pub type StreamChunkCallback =
    extern "C" fn(ctx: *mut c_void, data: *const c_char, data_len: usize);

/// Callback type for stream finish (new streaming architecture).
///
/// Called when PHP invokes `tokio_finish_request()` in streaming mode.
/// Simpler than FinishCallback - no body/headers passed (already sent via ub_write).
pub type StreamFinishCallback = extern "C" fn(ctx: *mut c_void);

#[link(name = "tokio_bridge")]
#[allow(dead_code)] // FFI functions may be called from C, not Rust
extern "C" {
    // Context lifecycle
    fn tokio_bridge_init_ctx(request_id: u64, worker_id: u64);
    fn tokio_bridge_destroy_ctx();
    fn tokio_bridge_get_ctx() -> *mut c_void;

    // Finish request
    fn tokio_bridge_is_finished() -> c_int;
    fn tokio_bridge_get_finished_offset() -> usize;
    fn tokio_bridge_get_finished_header_count() -> c_int;
    fn tokio_bridge_get_finished_response_code() -> c_int;

    // Heartbeat
    fn tokio_bridge_set_heartbeat(ctx: *mut c_void, max_secs: u64, callback: HeartbeatCallback);

    // Finish request callback (streaming early response)
    fn tokio_bridge_set_finish_callback(ctx: *mut c_void, callback: FinishCallback);

    // Streaming (SSE support)
    fn tokio_bridge_enable_streaming(ctx: *mut c_void, callback: StreamChunkCallback);
    fn tokio_bridge_set_stream_callback(ctx: *mut c_void, callback: StreamChunkCallback);
    fn tokio_bridge_try_enable_streaming() -> c_int;
    fn tokio_bridge_is_streaming() -> c_int;
    fn tokio_bridge_send_chunk(data: *const c_char, data_len: usize) -> c_int;
    fn tokio_bridge_get_stream_offset() -> usize;
    fn tokio_bridge_set_stream_offset(offset: usize);
    fn tokio_bridge_end_stream();

    // Stream finish (new streaming architecture)
    fn tokio_bridge_set_stream_finish_callback(ctx: *mut c_void, callback: StreamFinishCallback);
    fn tokio_bridge_trigger_stream_finish() -> c_int;
}

// =============================================================================
// Safe Rust API
// =============================================================================

/// Initialize the bridge context for the current thread.
///
/// Must be called before PHP execution starts.
#[inline]
pub fn init_ctx(request_id: u64, worker_id: u64) {
    unsafe {
        tokio_bridge_init_ctx(request_id, worker_id);
    }
}

/// Destroy the bridge context for the current thread.
///
/// Should be called after reading finish state and before the next request.
#[inline]
pub fn destroy_ctx() {
    unsafe {
        tokio_bridge_destroy_ctx();
    }
}

/// Check if the bridge context is initialized.
#[inline]
pub fn has_ctx() -> bool {
    unsafe { !tokio_bridge_get_ctx().is_null() }
}

/// Check if `tokio_finish_request()` was called.
#[inline]
pub fn is_finished() -> bool {
    unsafe { tokio_bridge_is_finished() != 0 }
}

/// Get the output byte offset where response should be truncated.
#[inline]
pub fn get_finished_offset() -> usize {
    unsafe { tokio_bridge_get_finished_offset() }
}

/// Get the number of headers at finish time.
#[inline]
pub fn get_finished_header_count() -> i32 {
    unsafe { tokio_bridge_get_finished_header_count() }
}

/// Get the HTTP response code at finish time.
#[inline]
pub fn get_finished_response_code() -> u16 {
    unsafe { tokio_bridge_get_finished_response_code() as u16 }
}

/// Set the heartbeat callback.
///
/// The callback will be invoked when PHP calls `tokio_request_heartbeat()`.
///
/// # Safety
///
/// `ctx` must be a valid pointer to a `HeartbeatContext` created via `Arc::as_ptr`.
#[inline]
pub unsafe fn set_heartbeat(ctx: *mut c_void, max_secs: u64, callback: HeartbeatCallback) {
    tokio_bridge_set_heartbeat(ctx, max_secs, callback);
}

/// Set the finish request callback.
///
/// The callback will be invoked when PHP calls `tokio_finish_request()`.
/// This enables streaming early response - client receives response immediately
/// while PHP continues background execution.
///
/// # Safety
///
/// `ctx` must be a valid pointer to a `FinishChannel` created via `Arc::as_ptr`.
#[inline]
pub unsafe fn set_finish_callback(ctx: *mut c_void, callback: FinishCallback) {
    tokio_bridge_set_finish_callback(ctx, callback);
}

/// Enable streaming mode for current request.
///
/// The callback will be invoked when PHP flushes output in streaming mode.
///
/// # Safety
///
/// `ctx` must be a valid pointer to a `StreamingChannel` created via `Arc::as_ptr`.
#[inline]
pub unsafe fn enable_streaming(ctx: *mut c_void, callback: StreamChunkCallback) {
    tokio_bridge_enable_streaming(ctx, callback);
}

/// Set streaming callback without enabling streaming mode.
///
/// This is used for all PHP requests to allow automatic SSE detection
/// via Content-Type header. Streaming is enabled later when PHP sets
/// `Content-Type: text/event-stream`.
///
/// # Safety
///
/// `ctx` must be a valid pointer to a `StreamingChannel` created via `Arc::as_ptr`.
#[inline]
pub unsafe fn set_stream_callback(ctx: *mut c_void, callback: StreamChunkCallback) {
    tokio_bridge_set_stream_callback(ctx, callback);
}

/// Try to enable streaming mode if callback is configured.
///
/// Returns `true` if streaming was enabled, `false` if no callback configured.
/// This is called from the header handler when Content-Type: text/event-stream is detected.
#[inline]
pub fn try_enable_streaming() -> bool {
    unsafe { tokio_bridge_try_enable_streaming() != 0 }
}

/// Check if streaming mode is enabled.
#[inline]
pub fn is_streaming() -> bool {
    unsafe { tokio_bridge_is_streaming() != 0 }
}

/// Get the current stream offset (for polling mode).
#[inline]
pub fn get_stream_offset() -> usize {
    unsafe { tokio_bridge_get_stream_offset() }
}

/// Set the stream offset (for polling mode).
#[inline]
pub fn set_stream_offset(offset: usize) {
    unsafe { tokio_bridge_set_stream_offset(offset) }
}

/// End streaming mode.
#[inline]
pub fn end_stream() {
    unsafe { tokio_bridge_end_stream() }
}

/// Set the stream finish callback.
///
/// In the new streaming architecture, this callback is invoked when PHP calls
/// `tokio_finish_request()`. Unlike the legacy FinishCallback, no body/headers
/// are passed because they've already been sent via ub_write.
///
/// # Safety
///
/// `ctx` must be a valid pointer that remains valid for the request lifetime,
/// or can be null if the callback doesn't need context.
#[inline]
pub unsafe fn set_stream_finish_callback(ctx: *mut c_void, callback: StreamFinishCallback) {
    tokio_bridge_set_stream_finish_callback(ctx, callback);
}

/// Trigger stream finish from PHP.
///
/// Called when PHP invokes `tokio_finish_request()`. Returns true if this was
/// the first finish call (callback invoked), false if already finished.
#[inline]
pub fn trigger_stream_finish() -> bool {
    unsafe { tokio_bridge_trigger_stream_finish() != 0 }
}

// =============================================================================
// Finish Channel (Streaming Early Response)
// =============================================================================

/// Data sent through the finish channel when PHP calls `tokio_finish_request()`.
#[derive(Debug, Clone)]
pub struct FinishData {
    /// Response body (output before finish_request).
    pub body: bytes::Bytes,
    /// Parsed HTTP headers (name, value pairs).
    pub headers: Vec<(String, String)>,
    /// HTTP response status code.
    pub status_code: u16,
}

/// Channel for receiving finish signal from PHP.
///
/// When PHP calls `tokio_finish_request()`, the callback sends response data
/// through this channel, allowing the HTTP handler to send the response immediately
/// while PHP continues executing in the background.
pub struct FinishChannel {
    tx: mpsc::Sender<FinishData>,
}

impl FinishChannel {
    /// Create a new finish channel.
    ///
    /// Returns the channel and a receiver. Capacity is 1 since only one
    /// finish signal can be sent per request.
    pub fn new() -> (Self, mpsc::Receiver<FinishData>) {
        let (tx, rx) = mpsc::channel(1);
        (Self { tx }, rx)
    }

    /// Get a raw pointer to this channel for passing to FFI.
    pub fn as_ptr(self: &Arc<Self>) -> *mut c_void {
        Arc::as_ptr(self) as *mut c_void
    }

    /// The FFI callback function that the bridge library invokes.
    ///
    /// Called from C when PHP calls `tokio_finish_request()`.
    /// Parses the response data and sends it through the channel.
    ///
    /// # Safety
    ///
    /// This is an FFI callback. The caller (C code) must ensure:
    /// - `ctx` is a valid pointer from `Arc::as_ptr`
    /// - `body` points to `body_len` bytes (or is null if body_len is 0)
    /// - `headers` points to `headers_len` bytes of serialized headers
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub extern "C" fn callback(
        ctx: *mut c_void,
        body: *const c_char,
        body_len: usize,
        headers: *const c_char,
        headers_len: usize,
        header_count: c_int,
        status_code: c_int,
    ) {
        if ctx.is_null() {
            return;
        }

        // SAFETY: ctx was created from Arc::as_ptr and is still valid
        let channel = unsafe { &*(ctx as *const FinishChannel) };

        // Copy body bytes
        let body_bytes = if body.is_null() || body_len == 0 {
            bytes::Bytes::new()
        } else {
            let slice = unsafe { std::slice::from_raw_parts(body.cast::<u8>(), body_len) };
            bytes::Bytes::copy_from_slice(slice)
        };

        // Parse headers from serialized buffer (name\0value\0name\0value\0...)
        let headers_vec = parse_headers_buffer(headers, headers_len, header_count);

        let data = FinishData {
            body: body_bytes,
            headers: headers_vec,
            status_code: status_code as u16,
        };

        // Non-blocking send (if receiver dropped, that's OK)
        let _ = channel.tx.try_send(data);
    }
}

impl Default for FinishChannel {
    fn default() -> Self {
        Self::new().0
    }
}

// =============================================================================
// Streaming Channel (SSE Support)
// =============================================================================

/// Channel for sending streaming chunks to the HTTP response.
///
/// When PHP flushes output in streaming mode, the callback sends data
/// through this channel, allowing the HTTP handler to stream chunks
/// to the client immediately.
pub struct StreamingChannel {
    tx: mpsc::Sender<StreamChunk>,
}

impl StreamingChannel {
    /// Create a new streaming channel.
    ///
    /// Returns the channel and a receiver. Buffer size controls backpressure.
    pub fn new(buffer_size: usize) -> (Self, mpsc::Receiver<StreamChunk>) {
        let (tx, rx) = mpsc::channel(buffer_size);
        (Self { tx }, rx)
    }

    /// Get a raw pointer to this channel for passing to FFI.
    pub fn as_ptr(self: &Arc<Self>) -> *mut c_void {
        Arc::as_ptr(self) as *mut c_void
    }

    /// The FFI callback function that the bridge library invokes.
    ///
    /// Called from C when PHP flushes output in streaming mode.
    /// Sends the chunk through the channel.
    ///
    /// # Safety
    ///
    /// This is an FFI callback. The caller (C code) must ensure:
    /// - `ctx` is a valid pointer from `Arc::as_ptr`
    /// - `data` points to `data_len` bytes (or is null if data_len is 0)
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub extern "C" fn callback(ctx: *mut c_void, data: *const c_char, data_len: usize) {
        if ctx.is_null() {
            return;
        }

        // SAFETY: ctx was created from Arc::as_ptr and is still valid
        let channel = unsafe { &*(ctx as *const StreamingChannel) };

        // Skip empty chunks
        if data.is_null() || data_len == 0 {
            return;
        }

        // Copy data bytes
        let slice = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), data_len) };
        let chunk = StreamChunk::from(slice);

        // Non-blocking send (if receiver dropped or buffer full, that's OK)
        let _ = channel.tx.try_send(chunk);
    }
}

/// Parse headers from serialized buffer format: name\0value\0name\0value\0...
fn parse_headers_buffer(ptr: *const c_char, len: usize, count: c_int) -> Vec<(String, String)> {
    if ptr.is_null() || len == 0 || count <= 0 {
        return Vec::new();
    }

    let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len) };
    let mut result = Vec::with_capacity(count as usize);
    let mut pos = 0;

    for _ in 0..count {
        if pos >= len {
            break;
        }

        // Read name (until \0)
        let name_start = pos;
        while pos < len && bytes[pos] != 0 {
            pos += 1;
        }
        let name = String::from_utf8_lossy(&bytes[name_start..pos]).into_owned();
        pos += 1; // Skip \0

        if pos >= len {
            break;
        }

        // Read value (until \0)
        let value_start = pos;
        while pos < len && bytes[pos] != 0 {
            pos += 1;
        }
        let value = String::from_utf8_lossy(&bytes[value_start..pos]).into_owned();
        pos += 1; // Skip \0

        result.push((name, value));
    }

    result
}

// =============================================================================
// Finish Request Info
// =============================================================================

/// Information captured when `tokio_finish_request()` is called.
#[derive(Debug, Clone)]
pub struct FinishRequestInfo {
    /// Byte offset in output where response ends.
    pub output_offset: usize,
    /// Number of headers set before finish.
    pub header_count: i32,
    /// HTTP response code.
    pub response_code: u16,
}

/// Check finish state and get info if finished.
///
/// Returns `Some(FinishRequestInfo)` if `tokio_finish_request()` was called,
/// `None` otherwise.
#[inline]
pub fn get_finish_info() -> Option<FinishRequestInfo> {
    if is_finished() {
        Some(FinishRequestInfo {
            output_offset: get_finished_offset(),
            header_count: get_finished_header_count(),
            response_code: get_finished_response_code(),
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_finish_channel_creation() {
        let (channel, _rx) = FinishChannel::new();
        let arc = Arc::new(channel);
        let ptr = arc.as_ptr();
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_parse_headers_buffer() {
        // Test empty buffer
        let result = parse_headers_buffer(std::ptr::null(), 0, 0);
        assert!(result.is_empty());

        // Test valid headers: "Content-Type\0text/html\0X-Test\0value\0"
        let buffer = b"Content-Type\0text/html\0X-Test\0value\0";
        let result = parse_headers_buffer(buffer.as_ptr() as *const c_char, buffer.len(), 2);
        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0],
            ("Content-Type".to_string(), "text/html".to_string())
        );
        assert_eq!(result[1], ("X-Test".to_string(), "value".to_string()));

        // Test with wrong count (more than actual)
        let result = parse_headers_buffer(buffer.as_ptr() as *const c_char, buffer.len(), 5);
        assert_eq!(result.len(), 2); // Should only parse what's available
    }

    #[test]
    fn test_streaming_channel_creation() {
        let (channel, _rx) = StreamingChannel::new(100);
        let arc = Arc::new(channel);
        let ptr = arc.as_ptr();
        assert!(!ptr.is_null());
    }
}
