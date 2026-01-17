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

thread_local! {
    /// Captured headers for current request
    pub static CAPTURED_HEADERS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    /// Captured HTTP status code
    pub static CAPTURED_STATUS: RefCell<u16> = const { RefCell::new(200) };
    /// Request data for SAPI callbacks (set before php_request_startup)
    static REQUEST_DATA: RefCell<Option<RequestDataOwned>> = const { RefCell::new(None) };
}

/// Custom header handler - captures headers set via header()
/// Stores headers in both Rust's CAPTURED_HEADERS and the bridge TLS (for PHP access)
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
                        buffer as *mut u8,
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
        // tokio_sapi extension loaded dynamically via php.ini
    }

    tracing::info!(
        "PHP initialized with SAPI 'cli-server' (register_server_variables, read_cookies, read_post)"
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
