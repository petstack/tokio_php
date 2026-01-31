//! PHP FFI bindings for the "tokio" SAPI implementation.
//!
//! This module provides manual FFI bindings to PHP's C API.
//! These bindings are designed for PHP 8.4+ ZTS builds.

#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::ffi::{c_char, c_int, c_uint, c_void};

// ============================================================================
// Basic Types
// ============================================================================

pub type zend_result = c_int;
pub type zend_long = i64;
pub type zend_ulong = u64;
pub type zend_bool = u8;

// stat structure (platform-specific)
#[cfg(target_os = "linux")]
pub type zend_stat_t = libc::stat;
#[cfg(target_os = "macos")]
pub type zend_stat_t = libc::stat;

// ============================================================================
// Zend Types
// ============================================================================

/// zend_refcounted_h - Reference counting header
#[repr(C)]
pub struct zend_refcounted_h {
    pub refcount: u32,
    pub type_info: u32,
}

/// zend_string - PHP string
#[repr(C)]
pub struct zend_string {
    pub gc: zend_refcounted_h,
    pub h: zend_ulong,
    pub len: usize,
    pub val: [c_char; 1], // Flexible array member
}

/// HashTable - PHP array (opaque)
#[repr(C)]
pub struct HashTable {
    _data: [u8; 56], // Opaque - don't access internals
}

/// zend_llist - Linked list (opaque)
#[repr(C)]
pub struct zend_llist {
    _data: [u8; 56], // Opaque
}

/// zval_value - Union of possible zval values
#[repr(C)]
pub union zval_value {
    pub lval: zend_long,
    pub dval: f64,
    pub counted: *mut zend_refcounted_h,
    pub str_: *mut zend_string,
    pub arr: *mut HashTable,
    pub obj: *mut c_void,
    pub res: *mut c_void,
    pub ref_: *mut c_void,
    pub ast: *mut c_void,
    pub zv: *mut zval,
    pub ptr: *mut c_void,
    pub ce: *mut c_void,
    pub func: *mut c_void,
    pub ww: zval_ww,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct zval_ww {
    pub w1: u32,
    pub w2: u32,
}

/// zval - PHP value container
#[repr(C)]
pub struct zval {
    pub value: zval_value,
    pub u1: zval_u1,
    pub u2: zval_u2,
}

#[repr(C)]
pub union zval_u1 {
    pub type_info: u32,
    pub v: zval_v,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct zval_v {
    pub type_: u8,
    pub type_flags: u8,
    pub extra: u16,
}

#[repr(C)]
pub union zval_u2 {
    pub next: u32,
    pub cache_slot: u32,
    pub opline_num: u32,
    pub lineno: u32,
    pub num_args: u32,
    pub fe_pos: u32,
    pub fe_iter_idx: u32,
    pub property_guard: u32,
    pub constant_flags: u32,
    pub extra: u32,
}

// zval type constants
pub const IS_UNDEF: u32 = 0;
pub const IS_NULL: u32 = 1;
pub const IS_FALSE: u32 = 2;
pub const IS_TRUE: u32 = 3;
pub const IS_LONG: u32 = 4;
pub const IS_DOUBLE: u32 = 5;
pub const IS_STRING: u32 = 6;
pub const IS_ARRAY: u32 = 7;
pub const IS_OBJECT: u32 = 8;
pub const IS_RESOURCE: u32 = 9;
pub const IS_REFERENCE: u32 = 10;

/// zend_execute_data - Execution context (opaque)
#[repr(C)]
pub struct zend_execute_data {
    _data: [u8; 128], // Opaque - accessed via macros
}

// ============================================================================
// SAPI Structures
// ============================================================================

/// sapi_header_struct - Individual HTTP header
#[repr(C)]
pub struct sapi_header_struct {
    pub header: *mut c_char,
    pub header_len: usize,
}

/// sapi_header_op_enum - Header operation types
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum sapi_header_op_enum {
    SAPI_HEADER_REPLACE = 0,
    SAPI_HEADER_ADD = 1,
    SAPI_HEADER_DELETE = 2,
    SAPI_HEADER_DELETE_PREFIX = 3,
    SAPI_HEADER_DELETE_ALL = 4,
    SAPI_HEADER_SET_STATUS = 5,
}

/// sapi_headers_struct - Response headers collection
#[repr(C)]
pub struct sapi_headers_struct {
    pub headers: zend_llist,
    pub http_response_code: c_int,
    pub send_default_content_type: u8,
    pub mimetype: *mut c_char,
    pub http_status_line: *mut c_char,
}

/// sapi_request_info - Request information
#[repr(C)]
pub struct sapi_request_info {
    pub request_method: *const c_char,
    pub query_string: *mut c_char,
    pub cookie_data: *mut c_char,
    pub content_length: zend_long,
    pub path_translated: *mut c_char,
    pub request_uri: *mut c_char,
    pub request_body: *mut c_void, // php_stream*
    pub content_type: *const c_char,
    pub headers_only: bool,
    pub no_headers: bool,
    pub headers_read: bool,
    pub post_entry: *mut c_void, // sapi_post_entry*
    pub content_type_dup: *mut c_char,
    pub auth_user: *mut c_char,
    pub auth_password: *mut c_char,
    pub auth_digest: *mut c_char,
    pub argv0: *mut c_char,
    pub current_user: *mut c_char,
    pub current_user_length: c_int,
    pub argc: c_int,
    pub argv: *mut *mut c_char,
    pub proto_num: c_int,
}

/// sapi_globals_struct - SAPI global state
#[repr(C)]
pub struct sapi_globals_struct {
    pub server_context: *mut c_void,
    pub request_info: sapi_request_info,
    pub sapi_headers: sapi_headers_struct,
    pub read_post_bytes: i64,
    pub post_read: u8,
    pub headers_sent: u8,
    pub global_stat: zend_stat_t,
    pub default_mimetype: *mut c_char,
    pub default_charset: *mut c_char,
    pub rfc1867_uploaded_files: *mut HashTable,
    pub post_max_size: zend_long,
    pub options: c_int,
    pub sapi_started: bool,
    pub global_request_time: f64,
    // Additional fields omitted for simplicity
}

/// sapi_module_struct - SAPI module definition
#[repr(C)]
pub struct sapi_module_struct {
    pub name: *mut c_char,
    pub pretty_name: *mut c_char,

    pub startup: Option<unsafe extern "C" fn(*mut sapi_module_struct) -> c_int>,
    pub shutdown: Option<unsafe extern "C" fn(*mut sapi_module_struct) -> c_int>,

    pub activate: Option<unsafe extern "C" fn() -> c_int>,
    pub deactivate: Option<unsafe extern "C" fn() -> c_int>,

    pub ub_write: Option<unsafe extern "C" fn(*const c_char, usize) -> usize>,
    pub flush: Option<unsafe extern "C" fn(*mut c_void)>,
    pub get_stat: Option<unsafe extern "C" fn() -> *mut zend_stat_t>,
    pub getenv: Option<unsafe extern "C" fn(*const c_char, usize) -> *mut c_char>,

    pub sapi_error: *mut c_void, // Variadic function pointer
    pub header_handler: Option<
        unsafe extern "C" fn(
            *mut sapi_header_struct,
            sapi_header_op_enum,
            *mut sapi_headers_struct,
        ) -> c_int,
    >,
    pub send_headers: Option<unsafe extern "C" fn(*mut sapi_headers_struct) -> c_int>,
    pub send_header: Option<unsafe extern "C" fn(*mut sapi_header_struct, *mut c_void)>,

    pub read_post: Option<unsafe extern "C" fn(*mut c_char, usize) -> usize>,
    pub read_cookies: Option<unsafe extern "C" fn() -> *mut c_char>,

    pub register_server_variables: Option<unsafe extern "C" fn(*mut zval)>,
    pub log_message: Option<unsafe extern "C" fn(*const c_char, c_int)>,
    pub get_request_time: Option<unsafe extern "C" fn(*mut f64) -> zend_result>,
    pub terminate_process: Option<unsafe extern "C" fn()>,

    // STANDARD_SAPI_MODULE_PROPERTIES
    pub php_ini_path_override: *mut c_char,
    pub default_post_reader: Option<unsafe extern "C" fn()>,
    pub treat_data: Option<unsafe extern "C" fn(c_int, *mut c_char, *mut zval)>,
    pub executable_location: *mut c_char,

    pub php_ini_ignore: c_int,
    pub php_ini_ignore_cwd: c_int,

    pub get_fd: Option<unsafe extern "C" fn(*mut c_int) -> c_int>,
    pub force_http_10: Option<unsafe extern "C" fn() -> c_int>,
    pub get_target_uid: Option<unsafe extern "C" fn(*mut libc::uid_t) -> c_int>,
    pub get_target_gid: Option<unsafe extern "C" fn(*mut libc::gid_t) -> c_int>,

    pub input_filter: Option<
        unsafe extern "C" fn(c_int, *const c_char, *mut *mut c_char, usize, *mut usize) -> c_uint,
    >,

    pub ini_defaults: Option<unsafe extern "C" fn(*mut HashTable)>,
    pub phpinfo_as_text: c_int,

    pub ini_entries: *const c_char,
    pub additional_functions: *const zend_function_entry,
    pub input_filter_init: Option<unsafe extern "C" fn() -> c_uint>,
    // Note: pre_request_init does NOT exist in PHP 8.4 sapi_module_struct
}

// ============================================================================
// Function Entry (for registering PHP functions)
// ============================================================================

/// Type alias for PHP internal function handler
pub type zif_handler = Option<unsafe extern "C" fn(*mut zend_execute_data, *mut zval)>;

/// zend_type - PHP 8.x type information (16 bytes on 64-bit)
#[repr(C)]
#[derive(Copy, Clone)]
pub struct zend_type {
    pub ptr: *mut c_void, // type_list for complex types
    pub type_mask: u32,   // MAY_BE_* flags
    _padding: u32,        // alignment padding
}

impl zend_type {
    /// Create an empty type (no type declaration)
    pub const fn none() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            type_mask: 0,
            _padding: 0,
        }
    }
}

/// zend_internal_arg_info - Argument info for internal functions
///
/// For the first element (return type info):
/// - `name` contains `(uintptr_t)required_num_args` cast to pointer
/// - `type_` contains return type info
///
/// For subsequent elements (argument info):
/// - `name` is the argument name
/// - `type_` is the argument type info
#[repr(C)]
pub struct zend_internal_arg_info {
    pub name: *const c_char,
    pub type_: zend_type,
    pub default_value: *const c_char,
}

impl zend_internal_arg_info {
    /// Create arginfo for a function with no arguments and no return type.
    pub const fn no_args() -> Self {
        Self {
            name: std::ptr::null(), // 0 required args
            type_: zend_type::none(),
            default_value: std::ptr::null(),
        }
    }

    /// Create arginfo for a function with N required arguments.
    pub const fn with_required_args(n: usize) -> Self {
        Self {
            name: n as *const c_char,
            type_: zend_type::none(),
            default_value: std::ptr::null(),
        }
    }
}

// SAFETY: zend_internal_arg_info is immutable static data
unsafe impl Sync for zend_internal_arg_info {}

/// zend_function_entry - Function registration entry
#[repr(C)]
pub struct zend_function_entry {
    pub fname: *const c_char,
    pub handler: zif_handler,
    pub arg_info: *const zend_internal_arg_info,
    pub num_args: u32,
    pub flags: u32,
    pub frameless_function_infos: *const c_void,
    pub doc_comment: *const c_char,
}

// Null terminator for function entry arrays
impl zend_function_entry {
    pub const NULL: Self = Self {
        fname: std::ptr::null(),
        handler: None,
        arg_info: std::ptr::null(),
        num_args: 0,
        flags: 0,
        frameless_function_infos: std::ptr::null(),
        doc_comment: std::ptr::null(),
    };
}

// SAFETY: zend_function_entry is immutable once initialized (static const data)
// and contains only raw pointers to static strings. PHP expects this array
// to be safely accessible from the main thread during module startup.
unsafe impl Sync for zend_function_entry {}

// ============================================================================
// File Handle (for script execution)
// ============================================================================

/// zend_file_handle - File handle for script execution
#[repr(C)]
pub struct zend_file_handle {
    pub handle: zend_file_handle_union,
    pub filename: *const zend_string,
    pub opened_path: *mut zend_string,
    pub type_: u8,
    pub primary_script: bool,
    pub in_list: bool,
    pub buf: *mut c_char,
    pub len: usize,
}

#[repr(C)]
pub union zend_file_handle_union {
    pub fp: *mut libc::FILE,
    pub stream: zend_stream,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct zend_stream {
    pub handle: *mut c_void,
    pub isatty: c_int,
    pub reader: *mut c_void,
    pub fsizer: *mut c_void,
    pub closer: *mut c_void,
}

// ============================================================================
// Constants
// ============================================================================

pub const SUCCESS: c_int = 0;
pub const FAILURE: c_int = -1;

// SAPI header constants
pub const SAPI_HEADER_SENT_SUCCESSFULLY: c_int = 1;
pub const SAPI_HEADER_DO_SEND: c_int = 2;
pub const SAPI_HEADER_SEND_FAILED: c_int = 3;

// Track vars indices (for PG(http_globals))
pub const TRACK_VARS_POST: usize = 0;
pub const TRACK_VARS_GET: usize = 1;
pub const TRACK_VARS_COOKIE: usize = 2;
pub const TRACK_VARS_SERVER: usize = 3;
pub const TRACK_VARS_ENV: usize = 4;
pub const TRACK_VARS_FILES: usize = 5;
pub const TRACK_VARS_REQUEST: usize = 6;

// ============================================================================
// External PHP Functions
// ============================================================================

#[link(name = "php")]
extern "C" {
    // TSRM startup (required for ZTS builds before sapi_startup)
    pub fn php_tsrm_startup() -> bool;

    // SAPI lifecycle
    pub fn sapi_startup(sf: *mut sapi_module_struct);
    pub fn sapi_shutdown();
    pub fn sapi_activate();
    pub fn sapi_deactivate();

    // SAPI globals (ZTS version - accessed via TSRMG offset)
    // Note: These offsets are defined in PHP and used to access thread-local globals
    pub static sapi_globals_id: c_int;

    // Module lifecycle
    pub fn php_module_startup(
        sf: *mut sapi_module_struct,
        additional_module: *mut c_void,
    ) -> zend_result;
    pub fn php_module_shutdown();

    // Request lifecycle
    pub fn php_request_startup() -> zend_result;
    pub fn php_request_shutdown(dummy: *mut c_void);

    // Script execution
    pub fn php_execute_script(primary_file: *mut zend_file_handle) -> zend_result;

    // File handle
    pub fn zend_stream_init_filename(handle: *mut zend_file_handle, filename: *const c_char);
    pub fn zend_destroy_file_handle(handle: *mut zend_file_handle);

    // Variable registration
    pub fn php_register_variable(
        var: *const c_char,
        val: *const c_char,
        track_vars_array: *mut zval,
    );
    pub fn php_register_variable_safe(
        var: *const c_char,
        val: *const c_char,
        val_len: usize,
        track_vars_array: *mut zval,
    );

    // Hash table operations
    pub fn zend_hash_str_find(ht: *mut HashTable, key: *const c_char, len: usize) -> *mut zval;
    pub fn zend_hash_str_update(
        ht: *mut HashTable,
        key: *const c_char,
        len: usize,
        pData: *mut zval,
    ) -> *mut zval;
    pub fn zend_hash_clean(ht: *mut HashTable);

    // String operations
    pub fn zend_string_init(str: *const c_char, len: usize, persistent: bool) -> *mut zend_string;
    pub fn zend_string_release(s: *mut zend_string);

    // Auto globals
    pub fn zend_is_auto_global(name: *mut zend_string) -> bool;
    pub fn zend_is_auto_global_str(name: *const c_char, len: usize) -> bool;

    // Output buffering
    pub fn php_output_start_default() -> c_int;
    pub fn php_output_get_level() -> c_int;
    pub fn php_output_flush() -> zend_result;
    pub fn php_output_end() -> zend_result;
    pub fn php_output_set_implicit_flush(flag: c_int);

    // Headers
    pub fn sapi_send_headers() -> c_int;

    // Parameter parsing
    pub fn zend_parse_parameters(num_args: u32, type_spec: *const c_char, ...) -> zend_result;

    // TSRM
    pub fn ts_resource_ex(id: c_int, th_id: *mut c_void) -> *mut c_void;

    // Errors
    pub fn php_error_docref(docref: *const c_char, type_: c_int, format: *const c_char, ...);

    // Global variables (ZTS version - we access via offset)
    pub static sapi_globals_offset: usize;
    pub static executor_globals_offset: usize;
}

// ============================================================================
// SAPI Globals Access (ZTS-safe)
// ============================================================================

/// Get pointer to sapi_globals for the current thread.
///
/// # Safety
/// Must be called from a PHP worker thread after TSRM initialization.
pub unsafe fn get_sapi_globals() -> *mut sapi_globals_struct {
    // In ZTS mode, sapi_globals is accessed via TSRMG offset
    let base = ts_resource_ex(0, std::ptr::null_mut()) as *mut u8;
    base.add(sapi_globals_offset) as *mut sapi_globals_struct
}

/// Set the request info fields in sapi_globals.
///
/// This must be called BEFORE php_request_startup() to enable
/// automatic $_GET and $_POST parsing.
///
/// # Safety
/// Must be called from a PHP worker thread.
pub unsafe fn set_request_info(
    method: *const c_char,
    query_string: *mut c_char,
    request_uri: *mut c_char,
    content_type: *const c_char,
    content_length: i64,
) {
    let sg = get_sapi_globals();
    if sg.is_null() {
        return;
    }

    (*sg).request_info.request_method = method;
    (*sg).request_info.query_string = query_string;
    (*sg).request_info.request_uri = request_uri;
    (*sg).request_info.content_type = content_type;
    (*sg).request_info.content_length = content_length;
}

// ============================================================================
// Helper Functions
// ============================================================================

impl zval {
    /// Create a new UNDEF zval
    pub const fn undef() -> Self {
        Self {
            value: zval_value { lval: 0 },
            u1: zval_u1 {
                type_info: IS_UNDEF,
            },
            u2: zval_u2 { extra: 0 },
        }
    }

    /// Set zval to long value
    pub fn set_long(&mut self, val: zend_long) {
        self.value.lval = val;
        self.u1.type_info = IS_LONG;
    }

    /// Set zval to bool value
    pub fn set_bool(&mut self, val: bool) {
        self.u1.type_info = if val { IS_TRUE } else { IS_FALSE };
    }

    /// Set zval to null
    pub fn set_null(&mut self) {
        self.u1.type_info = IS_NULL;
    }

    /// Get type info
    pub fn type_info(&self) -> u32 {
        unsafe { self.u1.type_info }
    }

    /// Get the long value (assumes type is IS_LONG)
    pub fn get_long(&self) -> zend_long {
        unsafe { self.value.lval }
    }
}

// ============================================================================
// Execute Data Access
// ============================================================================

/// Get the number of arguments passed to the current function.
///
/// This reads from EX(This).u2.num_args which is at a fixed offset
/// in the execute_data structure.
///
/// # Safety
/// Must be called from within a PHP internal function with valid execute_data.
pub unsafe fn get_num_args(execute_data: *const zend_execute_data) -> u32 {
    if execute_data.is_null() {
        return 0;
    }
    // In PHP 8.x, num_args is stored in This.u2.num_args
    // This is at offset 32 in zend_execute_data
    // u2 is at offset 12 within zval (after value union (8) + u1 (4))
    // Total offset: 32 + 12 = 44
    let ptr = (execute_data as *const u8).add(44) as *const u32;
    *ptr
}

/// Get a pointer to the first argument.
///
/// Arguments follow the zend_execute_data structure in memory.
///
/// # Safety
/// Must be called from within a PHP internal function with valid execute_data.
pub unsafe fn get_arg_ptr(execute_data: *const zend_execute_data, arg_num: u32) -> *const zval {
    if execute_data.is_null() {
        return std::ptr::null();
    }
    // Arguments start after zend_execute_data (size ~80 bytes in PHP 8.x)
    // Each argument is a zval (16 bytes)
    // arg_num is 1-based (first arg is 1)
    let base = (execute_data as *const u8).add(80) as *const zval;
    base.add((arg_num - 1) as usize)
}
