//! Custom SAPI initialization for PHP embed.
//!
//! This module provides alternative PHP initialization that can be extended
//! with custom SAPI callbacks in the future. Currently, it primarily sets
//! the SAPI name to 'cli-server' for OPcache compatibility.
//!
//! Note: PHP 8.4's output layer caches the ub_write callback during startup,
//! so custom output capture via SAPI callbacks doesn't work reliably.
//! Output is captured via stdout redirection instead.

use std::ffi::{c_char, c_int, CString};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

// =============================================================================
// PHP FFI Bindings
// =============================================================================

// Minimal SapiModule stub to access name field
#[repr(C)]
struct SapiModuleStub {
    name: *mut c_char,
    pretty_name: *mut c_char,
    // ... rest of fields not needed
}

#[link(name = "php")]
extern "C" {
    fn php_embed_init(argc: c_int, argv: *mut *mut c_char) -> c_int;
    fn php_embed_shutdown();

    // php_embed_module is the embed SAPI module - we modify before php_embed_init
    static mut php_embed_module: SapiModuleStub;
}

// =============================================================================
// SAPI Configuration
// =============================================================================

// Use cli-server for OPcache compatibility
static SAPI_NAME: &[u8] = b"cli-server\0";

static SAPI_INITIALIZED: AtomicBool = AtomicBool::new(false);

// =============================================================================
// Public API
// =============================================================================

/// Initialize PHP with custom SAPI settings (call once at startup)
pub fn init() -> Result<(), String> {
    if SAPI_INITIALIZED.swap(true, Ordering::SeqCst) {
        return Ok(()); // Already initialized
    }

    tracing::info!("sapi::init() - initializing PHP with cli-server SAPI name");

    unsafe {
        // Set SAPI name for OPcache compatibility
        php_embed_module.name = SAPI_NAME.as_ptr() as *mut c_char;

        let program_name = CString::new("tokio_php").unwrap();
        let mut argv: [*mut c_char; 2] = [program_name.as_ptr() as *mut c_char, ptr::null_mut()];

        if php_embed_init(1, argv.as_mut_ptr()) != 0 {
            return Err("Failed to initialize PHP embed".to_string());
        }
    }

    tracing::info!("PHP initialized with SAPI 'cli-server' (OPcache compatible)");
    Ok(())
}

/// Shutdown PHP
pub fn shutdown() {
    if !SAPI_INITIALIZED.swap(false, Ordering::SeqCst) {
        return; // Not initialized
    }

    unsafe {
        php_embed_shutdown();
    }

    tracing::info!("PHP shutdown complete");
}
