//! Custom SAPI initialization for PHP embed.
//!
//! This module provides PHP initialization with custom SAPI callbacks including
//! a header_handler to capture HTTP headers set via header().
//! Uses 'cli-server' SAPI name for OPcache compatibility.

use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

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
#[allow(dead_code)]
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
type HeaderHandlerFn = unsafe extern "C" fn(*mut SapiHeader, SapiHeaderOp, *mut SapiHeaders) -> c_int;
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
}

// =============================================================================
// Thread-local header storage
// =============================================================================

thread_local! {
    /// Captured headers for current request
    pub static CAPTURED_HEADERS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    /// Captured HTTP status code
    pub static CAPTURED_STATUS: RefCell<u16> = const { RefCell::new(200) };
}

/// Custom header handler - captures headers set via header()
unsafe extern "C" fn custom_header_handler(
    sapi_header: *mut SapiHeader,
    op: SapiHeaderOp,
    sapi_headers: *mut SapiHeaders,
) -> c_int {
    // Always check http_response_code from sapi_headers (set by header() third arg)
    if !sapi_headers.is_null() {
        let code = (*sapi_headers).http_response_code as u16;
        if code > 0 && code != 200 {
            CAPTURED_STATUS.with(|s| {
                *s.borrow_mut() = code;
            });
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
                        let name = name.trim().to_string();
                        let value = value.trim().to_string();

                        CAPTURED_HEADERS.with(|h| {
                            let mut headers = h.borrow_mut();
                            if op == SapiHeaderOp::Replace {
                                // Remove existing headers with same name (case-insensitive)
                                let name_lower = name.to_lowercase();
                                headers.retain(|(n, _)| n.to_lowercase() != name_lower);
                            }
                            headers.push((name, value));
                        });
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
                        h.borrow_mut().retain(|(n, _)| n.to_lowercase() != name_lower);
                    });
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

    tracing::info!("sapi::init() - initializing PHP with cli-server SAPI name");

    unsafe {
        // Set SAPI name for OPcache compatibility
        php_embed_module.name = SAPI_NAME.as_ptr() as *mut c_char;

        // Install custom header handler
        php_embed_module.header_handler = Some(custom_header_handler);

        let program_name = CString::new("tokio_php").unwrap();
        let mut argv: [*mut c_char; 2] = [program_name.as_ptr() as *mut c_char, ptr::null_mut()];

        if php_embed_init(1, argv.as_mut_ptr()) != 0 {
            return Err("Failed to initialize PHP embed".to_string());
        }
        // tokio_sapi extension loaded dynamically via php.ini
    }

    tracing::info!("PHP initialized with SAPI 'cli-server' (OPcache compatible, custom header handler)");
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
}

/// Get captured headers (call after request execution)
pub fn get_captured_headers() -> Vec<(String, String)> {
    CAPTURED_HEADERS.with(|h| h.borrow().clone())
}

/// Get captured HTTP status code
pub fn get_captured_status() -> u16 {
    CAPTURED_STATUS.with(|s| *s.borrow())
}
