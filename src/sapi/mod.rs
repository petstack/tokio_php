//! Custom PHP SAPI implementation in Rust.
//!
//! This module provides a pure Rust implementation of the PHP SAPI interface.
//! It replaces the need for the C-based tokio_sapi extension while providing
//! the same functionality.
//!
//! Note: SAPI name is "cli-server" for OPcache/JIT compatibility.
//! OPcache only enables for recognized SAPI names.
//!
//! # Features
//!
//! - Direct SAPI callback implementation in Rust
//! - PHP function registration via `additional_functions`
//! - Thread-local request context management
//! - Response streaming support
//!
//! # Usage
//!
//! ```rust,ignore
//! use tokio_php::sapi;
//!
//! // Initialize SAPI (once at startup)
//! sapi::init()?;
//!
//! // For each request:
//! sapi::context::init_context(request_id, worker_id);
//! sapi::context::set_request_data(server_vars, cookies, post_body);
//! sapi::callbacks::init_stream_state(response_tx);
//!
//! // Execute PHP...
//!
//! sapi::callbacks::finalize_stream();
//! sapi::context::clear_context();
//!
//! // Shutdown SAPI (once at shutdown)
//! sapi::shutdown();
//! ```
//!
//! # PHP Functions
//!
//! The following PHP functions are available when using this SAPI:
//!
//! - `tokio_request_id(): int` - Get unique request ID
//! - `tokio_worker_id(): int` - Get worker thread ID
//! - `tokio_server_info(): array` - Get server metadata
//! - `tokio_request_heartbeat(int $time = 10): bool` - Extend request timeout
//! - `tokio_finish_request(): bool` - Send response early, continue in background
//! - `tokio_send_headers(int $status = 200): bool` - Send headers, enable streaming
//! - `tokio_stream_flush(): bool` - Flush streaming output
//! - `tokio_is_streaming(): bool` - Check if streaming mode is active

pub mod callbacks;
pub mod context;
pub mod ffi;
pub mod functions;
pub mod module;

// Re-exports for convenience
pub use callbacks::{
    are_headers_sent, clear_captured_headers, clear_sapi_timing, finalize_stream,
    get_captured_headers, get_captured_status, get_sapi_timing, init_sapi_timing,
    init_stream_state, mark_stream_finished, set_status_code, ResponseChunk, SapiTiming,
    StreamState,
};
pub use context::{
    clear_context, init_context, register_temp_file, set_request_data, with_context,
    with_context_mut, RequestContext,
};
pub use ffi::set_request_info;
pub use module::{init, is_initialized, name, shutdown};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sapi_name() {
        assert_eq!(name(), "cli-server");
    }
}
