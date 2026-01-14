//! Rust bindings for tokio_bridge shared library.
//!
//! This module provides FFI bindings to the shared library that enables
//! direct communication between Rust and PHP, solving the TLS isolation
//! problem between statically linked Rust code and dynamically loaded PHP extensions.
//!
//! # Features
//!
//! - Shared TLS context accessible from both Rust and PHP
//! - Early Hints (HTTP 103) callback mechanism
//! - Finish request state (fastcgi_finish_request analog)
//! - Heartbeat for request timeout extension
//!
//! # Usage
//!
//! ```rust,ignore
//! use tokio_php::bridge;
//!
//! // Initialize context for a request
//! bridge::init_ctx(request_id, worker_id);
//!
//! // Set callbacks before PHP execution
//! bridge::set_hints_callback(channel_ptr, hints_callback);
//! bridge::set_heartbeat(heartbeat_ctx, max_secs, heartbeat_callback);
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

// =============================================================================
// FFI Bindings
// =============================================================================

/// Callback type for sending Early Hints.
pub type EarlyHintsCallback =
    extern "C" fn(ctx: *mut c_void, headers: *const *const c_char, count: usize);

/// Callback type for heartbeat (request timeout extension).
pub type HeartbeatCallback = extern "C" fn(ctx: *mut c_void, secs: u64) -> i64;

#[link(name = "tokio_bridge")]
extern "C" {
    // Context lifecycle
    fn tokio_bridge_init_ctx(request_id: u64, worker_id: u64);
    fn tokio_bridge_destroy_ctx();
    fn tokio_bridge_get_ctx() -> *mut c_void;

    // Early Hints
    fn tokio_bridge_set_hints_callback(ctx: *mut c_void, callback: EarlyHintsCallback);

    // Finish request
    fn tokio_bridge_is_finished() -> c_int;
    fn tokio_bridge_get_finished_offset() -> usize;
    fn tokio_bridge_get_finished_header_count() -> c_int;
    fn tokio_bridge_get_finished_response_code() -> c_int;

    // Heartbeat
    fn tokio_bridge_set_heartbeat(ctx: *mut c_void, max_secs: u64, callback: HeartbeatCallback);
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

/// Set the Early Hints callback.
///
/// The callback will be invoked when PHP calls `tokio_early_hints()`.
#[inline]
pub fn set_hints_callback(ctx: *mut c_void, callback: EarlyHintsCallback) {
    unsafe {
        tokio_bridge_set_hints_callback(ctx, callback);
    }
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
#[inline]
pub fn set_heartbeat(ctx: *mut c_void, max_secs: u64, callback: HeartbeatCallback) {
    unsafe {
        tokio_bridge_set_heartbeat(ctx, max_secs, callback);
    }
}

// =============================================================================
// Early Hints Channel
// =============================================================================

/// Channel for sending Early Hints from PHP to the HTTP handler.
///
/// This struct wraps an unbounded channel sender and provides the callback
/// that the bridge library invokes when PHP calls `tokio_early_hints()`.
pub struct EarlyHintsChannel {
    tx: mpsc::UnboundedSender<Vec<String>>,
}

impl EarlyHintsChannel {
    /// Create a new Early Hints channel.
    pub fn new() -> (Self, mpsc::UnboundedReceiver<Vec<String>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }

    /// Get a raw pointer to this channel for passing to FFI.
    pub fn as_ptr(self: &Arc<Self>) -> *mut c_void {
        Arc::as_ptr(self) as *mut c_void
    }

    /// The FFI callback function that the bridge library invokes.
    ///
    /// This function is called from C when PHP calls `tokio_early_hints()`.
    /// It converts the C string array to Rust strings and sends them through the channel.
    pub extern "C" fn callback(ctx: *mut c_void, headers: *const *const c_char, count: usize) {
        if ctx.is_null() || headers.is_null() || count == 0 {
            return;
        }

        // SAFETY: ctx was created from Arc::as_ptr and is still valid
        let channel = unsafe { &*(ctx as *const EarlyHintsChannel) };

        // Convert C strings to Rust strings
        let hints: Vec<String> = (0..count)
            .filter_map(|i| unsafe {
                let ptr = *headers.add(i);
                if ptr.is_null() {
                    return None;
                }
                std::ffi::CStr::from_ptr(ptr)
                    .to_str()
                    .ok()
                    .map(String::from)
            })
            .collect();

        if !hints.is_empty() {
            let _ = channel.tx.send(hints);
        }
    }
}

impl Default for EarlyHintsChannel {
    fn default() -> Self {
        Self::new().0
    }
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
    fn test_early_hints_channel_creation() {
        let (channel, _rx) = EarlyHintsChannel::new();
        let arc = Arc::new(channel);
        let ptr = arc.as_ptr();
        assert!(!ptr.is_null());
    }
}
