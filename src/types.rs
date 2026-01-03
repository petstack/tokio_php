//! Core types for script execution requests and responses.

use std::time::Duration;

use crate::profiler::ProfileData;

/// Key-value pair type for parameters (faster than HashMap for small collections).
pub type ParamList = Vec<(String, String)>;

// =============================================================================
// Uploaded File
// =============================================================================

/// Represents an uploaded file from multipart form data.
// Fields are only read by PHP executors (common.rs, ext.rs) which require the "php" feature.
#[derive(Debug, Clone)]
pub struct UploadedFile {
    /// Original filename
    #[cfg_attr(not(feature = "php"), allow(dead_code))]
    pub name: String,
    /// MIME type
    #[cfg_attr(not(feature = "php"), allow(dead_code))]
    pub mime_type: String,
    /// Temporary file path on disk
    pub tmp_name: String,
    /// File size in bytes
    #[cfg_attr(not(feature = "php"), allow(dead_code))]
    pub size: u64,
    /// PHP upload error code (0 = success)
    #[cfg_attr(not(feature = "php"), allow(dead_code))]
    pub error: u8,
}


// =============================================================================
// Script Request
// =============================================================================

/// Script execution request containing all HTTP request data.
// Fields are only read by PHP executors (common.rs, ext.rs) which require the "php" feature.
#[derive(Debug, Clone, Default)]
pub struct ScriptRequest {
    /// Path to the script file
    #[cfg_attr(not(feature = "php"), allow(dead_code))]
    pub script_path: String,
    /// GET parameters ($_GET)
    #[cfg_attr(not(feature = "php"), allow(dead_code))]
    pub get_params: ParamList,
    /// POST parameters ($_POST)
    #[cfg_attr(not(feature = "php"), allow(dead_code))]
    pub post_params: ParamList,
    /// Cookies ($_COOKIE)
    #[cfg_attr(not(feature = "php"), allow(dead_code))]
    pub cookies: ParamList,
    /// Server variables ($_SERVER)
    #[cfg_attr(not(feature = "php"), allow(dead_code))]
    pub server_vars: ParamList,
    /// Uploaded files ($_FILES)
    #[cfg_attr(not(feature = "php"), allow(dead_code))]
    pub files: Vec<(String, Vec<UploadedFile>)>,
    /// Raw request body for php://input (POST/QUERY methods)
    #[cfg_attr(not(feature = "php"), allow(dead_code))]
    pub raw_body: Option<Vec<u8>>,
    /// Enable profiling for this request
    #[cfg_attr(not(feature = "php"), allow(dead_code))]
    pub profile: bool,
    /// Request timeout (None = no timeout)
    #[cfg_attr(not(feature = "php"), allow(dead_code))]
    pub timeout: Option<Duration>,
}


// =============================================================================
// Script Response
// =============================================================================

/// Script execution response.
#[derive(Debug, Clone, Default)]
pub struct ScriptResponse {
    /// Response body
    pub body: String,
    /// Response headers
    pub headers: Vec<(String, String)>,
    /// Profiling data (if profiling was enabled)
    pub profile: Option<ProfileData>,
}

// =============================================================================
// Conversions to/from core types
// =============================================================================

impl From<ScriptResponse> for crate::core::Response {
    fn from(resp: ScriptResponse) -> Self {
        let mut builder = crate::core::Response::builder()
            .status(http::StatusCode::OK)
            .body(resp.body);

        for (name, value) in resp.headers {
            builder = builder.header(&name, value);
        }

        builder.build()
    }
}

