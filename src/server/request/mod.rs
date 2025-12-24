//! HTTP request parsing and context.

mod multipart;
mod parser;

use std::net::SocketAddr;
use std::sync::Arc;

use hyper::Uri;

use super::config::TlsInfo;
use crate::types::UploadedFile;

pub use multipart::parse_multipart;
pub use parser::{parse_cookies, parse_query_string};

/// Request context containing parsed request data.
///
/// This is a Parameter Object that consolidates multiple request parameters
/// into a single struct, reducing function argument count.
pub struct RequestContext {
    /// Remote client address
    pub remote_addr: SocketAddr,
    /// Request URI
    pub uri: Uri,
    /// HTTP version string (e.g., "HTTP/1.1", "HTTP/2.0")
    pub http_version: String,
    /// Request timestamp (seconds since epoch)
    pub request_time_secs: u64,
    /// Request timestamp (float with microseconds)
    pub request_time_float: f64,
    /// Whether profiling is enabled for this request
    pub profiling_enabled: bool,
    /// Whether client accepts Brotli compression
    pub use_brotli: bool,
    /// TLS connection info (if HTTPS)
    pub tls_info: Option<TlsInfo>,
    /// Document root path
    pub document_root: Arc<str>,
    /// Index file path (for single entry point mode)
    pub index_file_path: Option<Arc<str>>,
    /// Index file name (for blocking direct access)
    pub index_file_name: Option<Arc<str>>,
}

/// Parsed request data ready for script execution.
pub struct ParsedRequest {
    /// GET parameters
    pub get_params: Vec<(String, String)>,
    /// POST parameters
    pub post_params: Vec<(String, String)>,
    /// Cookies
    pub cookies: Vec<(String, String)>,
    /// Uploaded files
    pub files: Vec<(String, Vec<UploadedFile>)>,
    /// Server variables ($_SERVER)
    pub server_vars: Vec<(String, String)>,
    /// Resolved script path
    pub script_path: String,
}

/// Profile timing data for request parsing.
#[derive(Default)]
pub struct ParseTiming {
    pub headers_extract_us: u64,
    pub query_parse_us: u64,
    pub cookies_parse_us: u64,
    pub body_read_us: u64,
    pub body_parse_us: u64,
    pub server_vars_us: u64,
    pub path_resolve_us: u64,
    pub file_check_us: u64,
}
