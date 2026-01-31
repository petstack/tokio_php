//! SAPI module definition and lifecycle management.
//!
//! This module defines the SAPI module and provides
//! initialization and shutdown functions.
//!
//! Note: SAPI name is "cli-server" for OPcache/JIT compatibility.
//! OPcache only enables for recognized SAPI names.

use std::ffi::c_char;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use super::callbacks;
use super::ffi::*;
use super::functions::TOKIO_FUNCTIONS;

static SAPI_INITIALIZED: AtomicBool = AtomicBool::new(false);

// Static strings for SAPI name (must live for program duration)
// Using "cli-server" for OPcache/JIT compatibility (in OPcache's supported_sapis list)
static SAPI_NAME: &[u8] = b"cli-server\0";
static SAPI_PRETTY_NAME: &[u8] = b"Tokio PHP Server\0";

/// The SAPI module structure (name: "cli-server" for OPcache compatibility).
///
/// This is a static mut because PHP expects a mutable pointer to the module
/// during initialization. The module is only modified during init/shutdown.
#[allow(clippy::declare_interior_mutable_const)]
pub static mut TOKIO_SAPI_MODULE: sapi_module_struct = sapi_module_struct {
    name: SAPI_NAME.as_ptr() as *mut c_char,
    pretty_name: SAPI_PRETTY_NAME.as_ptr() as *mut c_char,

    // Lifecycle callbacks
    startup: Some(callbacks::sapi_startup_callback),
    shutdown: Some(callbacks::sapi_shutdown_callback),
    activate: Some(callbacks::sapi_activate_callback),
    deactivate: Some(callbacks::sapi_deactivate_callback),

    // Output callbacks
    ub_write: Some(callbacks::sapi_ub_write),
    flush: Some(callbacks::sapi_flush),
    get_stat: None,
    getenv: Some(callbacks::sapi_getenv),

    // Error handling
    sapi_error: ptr::null_mut(), // Use default

    // Header callbacks
    header_handler: Some(callbacks::sapi_header_handler),
    send_headers: Some(callbacks::sapi_send_headers),
    send_header: None,

    // Input callbacks
    read_post: Some(callbacks::sapi_read_post),
    read_cookies: Some(callbacks::sapi_read_cookies),

    // Server variables
    register_server_variables: Some(callbacks::sapi_register_server_variables),

    // Logging
    log_message: Some(callbacks::sapi_log_message),
    get_request_time: Some(callbacks::sapi_get_request_time),
    terminate_process: None,

    // STANDARD_SAPI_MODULE_PROPERTIES
    php_ini_path_override: ptr::null_mut(),
    default_post_reader: None,
    treat_data: None,
    executable_location: ptr::null_mut(),
    php_ini_ignore: 0,
    php_ini_ignore_cwd: 0,
    get_fd: None,
    force_http_10: None,
    get_target_uid: None,
    get_target_gid: None,
    input_filter: None,
    ini_defaults: None,
    phpinfo_as_text: 0,
    ini_entries: ptr::null(),
    additional_functions: TOKIO_FUNCTIONS.as_ptr(),
    input_filter_init: None,
    // Note: pre_request_init does NOT exist in PHP 8.4 sapi_module_struct
};

/// Initialize the SAPI module.
///
/// This function must be called once at server startup before any PHP execution.
/// It registers the SAPI module with PHP and initializes the PHP runtime.
///
/// # Errors
///
/// Returns an error if:
/// - SAPI is already initialized
/// - `sapi_startup` fails
/// - `php_module_startup` fails
pub fn init() -> Result<(), String> {
    if SAPI_INITIALIZED.swap(true, Ordering::SeqCst) {
        return Ok(()); // Already initialized
    }

    tracing::info!(sapi_name = "cli-server", "Initializing SAPI module");

    unsafe {
        // Initialize TSRM (Thread Safe Resource Manager) for ZTS builds
        // This MUST be called before sapi_startup() in ZTS builds
        if !php_tsrm_startup() {
            return Err("php_tsrm_startup failed".to_string());
        }

        // Register SAPI module with PHP
        sapi_startup(&raw mut TOKIO_SAPI_MODULE);

        // Initialize PHP module (PHP 8.4+ takes only 2 arguments)
        let result = php_module_startup(&raw mut TOKIO_SAPI_MODULE, ptr::null_mut());

        if result != SUCCESS {
            sapi_shutdown();
            SAPI_INITIALIZED.store(false, Ordering::SeqCst);
            return Err(format!("php_module_startup failed with code {}", result));
        }
    }

    tracing::info!(
        sapi_name = "cli-server",
        "SAPI module initialized successfully"
    );
    Ok(())
}

/// Shutdown the SAPI module.
///
/// This function should be called once at server shutdown.
/// It cleans up PHP resources and unregisters the SAPI module.
pub fn shutdown() {
    if !SAPI_INITIALIZED.swap(false, Ordering::SeqCst) {
        return; // Not initialized
    }

    tracing::info!(sapi_name = "cli-server", "Shutting down SAPI module");

    unsafe {
        php_module_shutdown();
        sapi_shutdown();
    }

    tracing::info!(sapi_name = "cli-server", "SAPI module shutdown complete");
}

/// Check if the SAPI is initialized.
pub fn is_initialized() -> bool {
    SAPI_INITIALIZED.load(Ordering::SeqCst)
}

/// Get the SAPI name.
pub fn name() -> &'static str {
    "cli-server"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sapi_name() {
        assert_eq!(name(), "cli-server");
    }

    #[test]
    fn test_static_strings() {
        // Verify null termination
        assert_eq!(SAPI_NAME.last(), Some(&0u8));
        assert_eq!(SAPI_PRETTY_NAME.last(), Some(&0u8));
    }
}
