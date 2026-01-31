//! PHP function implementations for the "tokio" SAPI.
//!
//! This module provides the PHP functions that are available in user scripts:
//! - tokio_request_id()
//! - tokio_worker_id()
//! - tokio_server_info()
//! - tokio_request_heartbeat($time)
//! - tokio_finish_request()
//! - tokio_send_headers($status)
//! - tokio_stream_flush()
//! - tokio_is_streaming()

use std::ffi::c_char;
use std::ptr;

use super::callbacks;
use super::ffi::*;
use crate::bridge;

// ============================================================================
// PHP Function Implementations
// ============================================================================

/// tokio_request_id(): int
///
/// Returns the unique request ID for the current request.
unsafe extern "C" fn php_tokio_request_id(
    _execute_data: *mut zend_execute_data,
    return_value: *mut zval,
) {
    let request_id = super::context::with_context(|ctx| ctx.request_id()).unwrap_or(0);
    (*return_value).set_long(request_id as zend_long);
}

/// tokio_worker_id(): int
///
/// Returns the worker ID that is processing the current request.
unsafe extern "C" fn php_tokio_worker_id(
    _execute_data: *mut zend_execute_data,
    return_value: *mut zval,
) {
    let worker_id = super::context::with_context(|ctx| ctx.worker_id()).unwrap_or(0);
    (*return_value).set_long(worker_id as zend_long);
}

/// tokio_server_info(): array
///
/// Returns an array with server information.
/// Note: This is a simplified implementation that returns basic info.
unsafe extern "C" fn php_tokio_server_info(
    _execute_data: *mut zend_execute_data,
    return_value: *mut zval,
) {
    // For now, return an empty array
    // Full implementation would create an array with:
    // - 'sapi' => 'tokio'
    // - 'version' => PKG_VERSION
    // - 'request_id' => ...
    // - 'worker_id' => ...
    // - 'build' => BUILD_VERSION
    (*return_value).set_null(); // TODO: implement array creation
}

/// tokio_request_heartbeat(int $time = 10): bool
///
/// Extends the request timeout deadline by the specified number of seconds.
unsafe extern "C" fn php_tokio_request_heartbeat(
    _execute_data: *mut zend_execute_data,
    return_value: *mut zval,
) {
    // TODO: Parse $time parameter from execute_data
    // For now, use default of 10 seconds
    let time: u64 = 10;

    let result = bridge::send_heartbeat(time);
    (*return_value).set_bool(result);
}

/// tokio_finish_request(): bool
///
/// Sends the response to the client immediately and continues executing
/// the script in the background. Analog of fastcgi_finish_request().
unsafe extern "C" fn php_tokio_finish_request(
    _execute_data: *mut zend_execute_data,
    return_value: *mut zval,
) {
    // Check if already finished
    if bridge::is_finished() {
        (*return_value).set_bool(true);
        return;
    }

    // Flush output buffers
    while php_output_get_level() > 0 {
        let _ = php_output_end();
    }

    // Trigger stream finish via callback
    let result = callbacks::mark_stream_finished();

    // Mark as finished in bridge
    bridge::mark_finished();

    // Start new buffer for post-finish output (will be discarded)
    php_output_start_default();

    (*return_value).set_bool(result);
}

/// tokio_send_headers(int $status = 200): bool
///
/// Sends HTTP headers immediately and enables chunked streaming mode.
unsafe extern "C" fn php_tokio_send_headers(
    _execute_data: *mut zend_execute_data,
    return_value: *mut zval,
) {
    // TODO: Parse $status parameter from execute_data
    // let status: u16 = 200;

    // Check if headers already sent
    if bridge::are_headers_sent() {
        (*return_value).set_bool(false);
        return;
    }

    // Disable output buffering
    while php_output_get_level() > 0 {
        let _ = php_output_end();
    }

    // Enable chunked mode in bridge
    bridge::enable_chunked_mode();

    // Send headers via SAPI
    let result = sapi_send_headers();
    if result != SAPI_HEADER_SENT_SUCCESSFULLY {
        (*return_value).set_bool(false);
        return;
    }

    // Enable implicit flush for subsequent output
    php_output_set_implicit_flush(1);

    (*return_value).set_bool(true);
}

/// tokio_stream_flush(): bool
///
/// Flushes output buffer and sends data to client immediately in streaming mode.
unsafe extern "C" fn php_tokio_stream_flush(
    _execute_data: *mut zend_execute_data,
    return_value: *mut zval,
) {
    // Check if streaming mode is enabled
    if !bridge::is_streaming() {
        (*return_value).set_bool(false);
        return;
    }

    // Flush all output buffers
    while php_output_get_level() > 0 {
        let _ = php_output_flush();
    }

    (*return_value).set_bool(true);
}

/// tokio_is_streaming(): bool
///
/// Returns true if streaming mode is currently enabled.
unsafe extern "C" fn php_tokio_is_streaming(
    _execute_data: *mut zend_execute_data,
    return_value: *mut zval,
) {
    let is_streaming = bridge::is_streaming();
    (*return_value).set_bool(is_streaming);
}

/// tokio_http_response_code(int $code = 0): int|bool
///
/// Get or set the HTTP response code. Works like PHP's http_response_code()
/// but stores the value in the bridge context for reliable access.
///
/// When called with no argument (or 0), returns the current status code.
/// When called with a code, sets the status code and returns the previous one.
unsafe extern "C" fn php_tokio_http_response_code(
    execute_data: *mut zend_execute_data,
    return_value: *mut zval,
) {
    // Get the current status code from CAPTURED_STATUS
    let current_code = callbacks::get_captured_status();

    // Get argument using our helper functions
    let num_args = get_num_args(execute_data);
    let code = if num_args > 0 {
        let arg = get_arg_ptr(execute_data, 1);
        if !arg.is_null() {
            let type_info = (*arg).type_info();
            if type_info == IS_LONG {
                let val = (*arg).get_long();
                if (100..600).contains(&val) {
                    Some(val as u16)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    if let Some(code) = code {
        // Set new status code
        bridge::set_response_code(code);
        callbacks::set_status_code(code);

        // Return the previous status code
        (*return_value).set_long(current_code as zend_long);
    } else {
        // Return current status code
        (*return_value).set_long(current_code as zend_long);
    }
}

// ============================================================================
// Function Registration
// ============================================================================

// Static function names (null-terminated)
static FNAME_REQUEST_ID: &[u8] = b"tokio_request_id\0";
static FNAME_WORKER_ID: &[u8] = b"tokio_worker_id\0";
static FNAME_SERVER_INFO: &[u8] = b"tokio_server_info\0";
static FNAME_HEARTBEAT: &[u8] = b"tokio_request_heartbeat\0";
static FNAME_FINISH: &[u8] = b"tokio_finish_request\0";
static FNAME_SEND_HEADERS: &[u8] = b"tokio_send_headers\0";
static FNAME_STREAM_FLUSH: &[u8] = b"tokio_stream_flush\0";
static FNAME_IS_STREAMING: &[u8] = b"tokio_is_streaming\0";
static FNAME_HTTP_RESPONSE_CODE: &[u8] = b"tokio_http_response_code\0";

// Arginfo for functions (required by PHP 8.x to avoid warnings)
// Each array has 1 element for return info + N elements for arguments
// num_args = array_len - 1

/// Arginfo for functions with no arguments
static ARGINFO_NO_ARGS: [zend_internal_arg_info; 1] = [zend_internal_arg_info::no_args()];

/// Arginfo for tokio_request_heartbeat(int $time = 10) - 0 required, 1 optional
static ARGINFO_HEARTBEAT: [zend_internal_arg_info; 2] = [
    zend_internal_arg_info::no_args(), // 0 required args
    zend_internal_arg_info {
        name: c"time".as_ptr(),
        type_: zend_type::none(),
        default_value: c"10".as_ptr(),
    },
];

/// Arginfo for tokio_send_headers(int $status = 200) - 0 required, 1 optional
static ARGINFO_SEND_HEADERS: [zend_internal_arg_info; 2] = [
    zend_internal_arg_info::no_args(), // 0 required args
    zend_internal_arg_info {
        name: c"status".as_ptr(),
        type_: zend_type::none(),
        default_value: c"200".as_ptr(),
    },
];

/// Arginfo for tokio_http_response_code(int $code = 0) - 0 required, 1 optional
static ARGINFO_HTTP_RESPONSE_CODE: [zend_internal_arg_info; 2] = [
    zend_internal_arg_info::no_args(), // 0 required args
    zend_internal_arg_info {
        name: c"code".as_ptr(),
        type_: zend_type::none(),
        default_value: c"0".as_ptr(),
    },
];

/// Function table for additional_functions in sapi_module_struct.
///
/// These functions become available to PHP scripts when using the tokio SAPI.
pub static TOKIO_FUNCTIONS: [zend_function_entry; 10] = [
    zend_function_entry {
        fname: FNAME_REQUEST_ID.as_ptr() as *const c_char,
        handler: Some(php_tokio_request_id),
        arg_info: ARGINFO_NO_ARGS.as_ptr(),
        num_args: 0, // array_len(1) - 1 = 0
        flags: 0,
        frameless_function_infos: ptr::null(),
        doc_comment: ptr::null(),
    },
    zend_function_entry {
        fname: FNAME_WORKER_ID.as_ptr() as *const c_char,
        handler: Some(php_tokio_worker_id),
        arg_info: ARGINFO_NO_ARGS.as_ptr(),
        num_args: 0,
        flags: 0,
        frameless_function_infos: ptr::null(),
        doc_comment: ptr::null(),
    },
    zend_function_entry {
        fname: FNAME_SERVER_INFO.as_ptr() as *const c_char,
        handler: Some(php_tokio_server_info),
        arg_info: ARGINFO_NO_ARGS.as_ptr(),
        num_args: 0,
        flags: 0,
        frameless_function_infos: ptr::null(),
        doc_comment: ptr::null(),
    },
    zend_function_entry {
        fname: FNAME_HEARTBEAT.as_ptr() as *const c_char,
        handler: Some(php_tokio_request_heartbeat),
        arg_info: ARGINFO_HEARTBEAT.as_ptr(),
        num_args: 1, // array_len(2) - 1 = 1
        flags: 0,
        frameless_function_infos: ptr::null(),
        doc_comment: ptr::null(),
    },
    zend_function_entry {
        fname: FNAME_FINISH.as_ptr() as *const c_char,
        handler: Some(php_tokio_finish_request),
        arg_info: ARGINFO_NO_ARGS.as_ptr(),
        num_args: 0,
        flags: 0,
        frameless_function_infos: ptr::null(),
        doc_comment: ptr::null(),
    },
    zend_function_entry {
        fname: FNAME_SEND_HEADERS.as_ptr() as *const c_char,
        handler: Some(php_tokio_send_headers),
        arg_info: ARGINFO_SEND_HEADERS.as_ptr(),
        num_args: 1, // array_len(2) - 1 = 1
        flags: 0,
        frameless_function_infos: ptr::null(),
        doc_comment: ptr::null(),
    },
    zend_function_entry {
        fname: FNAME_STREAM_FLUSH.as_ptr() as *const c_char,
        handler: Some(php_tokio_stream_flush),
        arg_info: ARGINFO_NO_ARGS.as_ptr(),
        num_args: 0,
        flags: 0,
        frameless_function_infos: ptr::null(),
        doc_comment: ptr::null(),
    },
    zend_function_entry {
        fname: FNAME_IS_STREAMING.as_ptr() as *const c_char,
        handler: Some(php_tokio_is_streaming),
        arg_info: ARGINFO_NO_ARGS.as_ptr(),
        num_args: 0,
        flags: 0,
        frameless_function_infos: ptr::null(),
        doc_comment: ptr::null(),
    },
    zend_function_entry {
        fname: FNAME_HTTP_RESPONSE_CODE.as_ptr() as *const c_char,
        handler: Some(php_tokio_http_response_code),
        arg_info: ARGINFO_HTTP_RESPONSE_CODE.as_ptr(),
        num_args: 1, // array_len(2) - 1 = 1
        flags: 0,
        frameless_function_infos: ptr::null(),
        doc_comment: ptr::null(),
    },
    // Null terminator (required by PHP)
    zend_function_entry::NULL,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_table_size() {
        // 9 functions + 1 null terminator
        assert_eq!(TOKIO_FUNCTIONS.len(), 10);
    }

    #[test]
    fn test_function_names_null_terminated() {
        assert_eq!(FNAME_REQUEST_ID.last(), Some(&0u8));
        assert_eq!(FNAME_WORKER_ID.last(), Some(&0u8));
        assert_eq!(FNAME_SERVER_INFO.last(), Some(&0u8));
        assert_eq!(FNAME_HEARTBEAT.last(), Some(&0u8));
        assert_eq!(FNAME_FINISH.last(), Some(&0u8));
        assert_eq!(FNAME_SEND_HEADERS.last(), Some(&0u8));
        assert_eq!(FNAME_STREAM_FLUSH.last(), Some(&0u8));
        assert_eq!(FNAME_IS_STREAMING.last(), Some(&0u8));
        assert_eq!(FNAME_HTTP_RESPONSE_CODE.last(), Some(&0u8));
    }

    #[test]
    fn test_null_terminator() {
        let last = &TOKIO_FUNCTIONS[9];
        assert!(last.fname.is_null());
        assert!(last.handler.is_none());
    }
}
