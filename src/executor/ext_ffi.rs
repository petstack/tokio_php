//! FFI bindings to tokio_sapi PHP extension.
//!
//! This module provides Rust bindings to the C functions exported by
//! the tokio_sapi PHP extension, enabling direct manipulation of
//! PHP superglobals without eval().

use std::ffi::{c_char, c_int, CString};
use std::sync::atomic::{AtomicBool, Ordering};

// =============================================================================
// FFI Function Declarations
// =============================================================================

#[link(name = "php")]
extern "C" {
    // Extension lifecycle
    fn tokio_sapi_init() -> c_int;
    fn tokio_sapi_shutdown();

    // Request lifecycle
    fn tokio_sapi_request_init(request_id: u64) -> c_int;
    fn tokio_sapi_request_shutdown();

    // POST body
    fn tokio_sapi_set_post_data(data: *const c_char, len: usize);

    // Superglobals (the main performance win!)
    fn tokio_sapi_set_server_var(
        key: *const c_char, key_len: usize,
        value: *const c_char, value_len: usize,
    );
    fn tokio_sapi_set_get_var(
        key: *const c_char, key_len: usize,
        value: *const c_char, value_len: usize,
    );
    fn tokio_sapi_set_post_var(
        key: *const c_char, key_len: usize,
        value: *const c_char, value_len: usize,
    );
    fn tokio_sapi_set_cookie_var(
        key: *const c_char, key_len: usize,
        value: *const c_char, value_len: usize,
    );
    fn tokio_sapi_set_files_var(
        field: *const c_char, field_len: usize,
        name: *const c_char,
        mime_type: *const c_char,
        tmp_name: *const c_char,
        error: c_int,
        size: usize,
    );
    fn tokio_sapi_clear_superglobals();
    fn tokio_sapi_build_request();

    // Output capture
    fn tokio_sapi_start_output_capture();
    fn tokio_sapi_get_output(len: *mut usize) -> *const c_char;
    fn tokio_sapi_clear_output();

    // Headers
    fn tokio_sapi_get_header_count() -> c_int;
    fn tokio_sapi_get_header_name(index: c_int) -> *const c_char;
    fn tokio_sapi_get_header_value(index: c_int) -> *const c_char;
    fn tokio_sapi_get_response_code() -> c_int;

    // Script execution
    fn tokio_sapi_execute_script(path: *const c_char) -> c_int;
}

// =============================================================================
// Initialization State
// =============================================================================

static EXTENSION_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// Check if the extension is available
pub fn is_available() -> bool {
    EXTENSION_AVAILABLE.load(Ordering::Relaxed)
}

/// Try to initialize the extension
pub fn init() -> Result<(), String> {
    // The extension is loaded automatically by PHP
    // We just need to verify it's available by checking if functions work

    // For now, assume it's available if we get here
    // In production, we'd check if the extension is loaded
    EXTENSION_AVAILABLE.store(true, Ordering::Relaxed);

    tracing::info!("tokio_sapi extension FFI bindings initialized");
    Ok(())
}

/// Shutdown the extension
pub fn shutdown() {
    if is_available() {
        unsafe {
            tokio_sapi_shutdown();
        }
        EXTENSION_AVAILABLE.store(false, Ordering::Relaxed);
    }
}

// =============================================================================
// Request Lifecycle
// =============================================================================

/// Initialize a new request context
pub fn request_init(request_id: u64) -> Result<(), String> {
    if !is_available() {
        return Err("Extension not available".to_string());
    }

    let result = unsafe { tokio_sapi_request_init(request_id) };
    if result == 0 {
        Ok(())
    } else {
        Err("Failed to initialize request".to_string())
    }
}

/// Shutdown request context
pub fn request_shutdown() {
    if is_available() {
        unsafe {
            tokio_sapi_request_shutdown();
        }
    }
}

// =============================================================================
// Superglobals (Zero-copy where possible)
// =============================================================================

/// Set a $_SERVER variable
#[inline]
pub fn set_server_var(key: &str, value: &str) {
    if !is_available() {
        return;
    }
    unsafe {
        tokio_sapi_set_server_var(
            key.as_ptr() as *const c_char,
            key.len(),
            value.as_ptr() as *const c_char,
            value.len(),
        );
    }
}

/// Set a $_GET variable
#[inline]
pub fn set_get_var(key: &str, value: &str) {
    if !is_available() {
        return;
    }
    unsafe {
        tokio_sapi_set_get_var(
            key.as_ptr() as *const c_char,
            key.len(),
            value.as_ptr() as *const c_char,
            value.len(),
        );
    }
}

/// Set a $_POST variable
#[inline]
pub fn set_post_var(key: &str, value: &str) {
    if !is_available() {
        return;
    }
    unsafe {
        tokio_sapi_set_post_var(
            key.as_ptr() as *const c_char,
            key.len(),
            value.as_ptr() as *const c_char,
            value.len(),
        );
    }
}

/// Set a $_COOKIE variable
#[inline]
pub fn set_cookie_var(key: &str, value: &str) {
    if !is_available() {
        return;
    }
    unsafe {
        tokio_sapi_set_cookie_var(
            key.as_ptr() as *const c_char,
            key.len(),
            value.as_ptr() as *const c_char,
            value.len(),
        );
    }
}

/// Set a $_FILES entry
pub fn set_files_var(
    field: &str,
    name: &str,
    mime_type: &str,
    tmp_name: &str,
    error: i32,
    size: usize,
) {
    if !is_available() {
        return;
    }

    // Need null-terminated strings for these
    let name_c = CString::new(name).unwrap_or_default();
    let type_c = CString::new(mime_type).unwrap_or_default();
    let tmp_c = CString::new(tmp_name).unwrap_or_default();

    unsafe {
        tokio_sapi_set_files_var(
            field.as_ptr() as *const c_char,
            field.len(),
            name_c.as_ptr(),
            type_c.as_ptr(),
            tmp_c.as_ptr(),
            error,
            size,
        );
    }
}

/// Clear all superglobals
#[inline]
pub fn clear_superglobals() {
    if !is_available() {
        return;
    }
    unsafe {
        tokio_sapi_clear_superglobals();
    }
}

/// Build $_REQUEST from $_GET + $_POST
#[inline]
pub fn build_request() {
    if !is_available() {
        return;
    }
    unsafe {
        tokio_sapi_build_request();
    }
}

// =============================================================================
// POST Body
// =============================================================================

/// Set POST body data for php://input
pub fn set_post_data(data: &[u8]) {
    if !is_available() {
        return;
    }
    unsafe {
        tokio_sapi_set_post_data(data.as_ptr() as *const c_char, data.len());
    }
}

// =============================================================================
// Output Capture
// =============================================================================

/// Start capturing output
#[inline]
pub fn start_output_capture() {
    if !is_available() {
        return;
    }
    unsafe {
        tokio_sapi_start_output_capture();
    }
}

/// Get captured output
pub fn get_output() -> String {
    if !is_available() {
        return String::new();
    }

    let mut len: usize = 0;
    let ptr = unsafe { tokio_sapi_get_output(&mut len) };

    if ptr.is_null() || len == 0 {
        return String::new();
    }

    unsafe {
        let slice = std::slice::from_raw_parts(ptr as *const u8, len);
        String::from_utf8_lossy(slice).into_owned()
    }
}

/// Clear captured output
#[inline]
pub fn clear_output() {
    if !is_available() {
        return;
    }
    unsafe {
        tokio_sapi_clear_output();
    }
}

// =============================================================================
// Headers
// =============================================================================

/// Get all captured headers
pub fn get_headers() -> Vec<(String, String)> {
    if !is_available() {
        return Vec::new();
    }

    let count = unsafe { tokio_sapi_get_header_count() };
    let mut headers = Vec::with_capacity(count as usize);

    for i in 0..count {
        let name_ptr = unsafe { tokio_sapi_get_header_name(i) };
        let value_ptr = unsafe { tokio_sapi_get_header_value(i) };

        if !name_ptr.is_null() && !value_ptr.is_null() {
            let name = unsafe {
                std::ffi::CStr::from_ptr(name_ptr)
                    .to_string_lossy()
                    .into_owned()
            };
            let value = unsafe {
                std::ffi::CStr::from_ptr(value_ptr)
                    .to_string_lossy()
                    .into_owned()
            };
            headers.push((name, value));
        }
    }

    headers
}

/// Get HTTP response code
#[inline]
pub fn get_response_code() -> u16 {
    if !is_available() {
        return 200;
    }
    unsafe { tokio_sapi_get_response_code() as u16 }
}

// =============================================================================
// Script Execution
// =============================================================================

/// Execute a PHP script
pub fn execute_script(path: &str) -> Result<(), String> {
    if !is_available() {
        return Err("Extension not available".to_string());
    }

    let path_c = CString::new(path).map_err(|e| e.to_string())?;

    let result = unsafe { tokio_sapi_execute_script(path_c.as_ptr()) };

    if result == 0 {
        Ok(())
    } else {
        Err("Script execution failed".to_string())
    }
}

// =============================================================================
// High-Level API: Set all superglobals from ScriptRequest
// =============================================================================

use crate::types::ScriptRequest;

/// Set all superglobals from a ScriptRequest (replaces eval-based approach)
pub fn set_superglobals_from_request(request: &ScriptRequest) {
    if !is_available() {
        return;
    }

    // Clear previous values
    clear_superglobals();

    // $_SERVER
    for (key, value) in &request.server_vars {
        set_server_var(key, value);
    }

    // $_GET
    for (key, value) in &request.get_params {
        set_get_var(key, value);
    }

    // $_POST
    for (key, value) in &request.post_params {
        set_post_var(key, value);
    }

    // $_COOKIE
    for (key, value) in &request.cookies {
        set_cookie_var(key, value);
    }

    // $_FILES
    for (field, files) in &request.files {
        for file in files {
            set_files_var(
                field,
                &file.name,
                &file.mime_type,
                &file.tmp_name,
                file.error as i32,
                file.size as usize,
            );
        }
    }

    // Build $_REQUEST
    build_request();
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_check() {
        // Just verify the module compiles correctly
        assert!(!is_available());
    }
}
