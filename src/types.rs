//! Core types for script execution requests and responses.

use crate::profiler::ProfileData;

/// Key-value pair type for parameters (faster than HashMap for small collections).
pub type ParamList = Vec<(String, String)>;

// =============================================================================
// Uploaded File
// =============================================================================

/// Represents an uploaded file from multipart form data.
#[derive(Debug, Clone)]
pub struct UploadedFile {
    /// Original filename
    pub name: String,
    /// MIME type
    pub mime_type: String,
    /// Temporary file path on disk
    pub tmp_name: String,
    /// File size in bytes
    pub size: u64,
    /// PHP upload error code (0 = success)
    pub error: u8,
}


// =============================================================================
// Script Request
// =============================================================================

/// Script execution request containing all HTTP request data.
#[derive(Debug, Clone, Default)]
pub struct ScriptRequest {
    /// Path to the script file
    pub script_path: String,
    /// GET parameters ($_GET)
    pub get_params: ParamList,
    /// POST parameters ($_POST)
    pub post_params: ParamList,
    /// Cookies ($_COOKIE)
    pub cookies: ParamList,
    /// Server variables ($_SERVER)
    pub server_vars: ParamList,
    /// Uploaded files ($_FILES)
    pub files: Vec<(String, Vec<UploadedFile>)>,
    /// Enable profiling for this request
    pub profile: bool,
}

impl ScriptRequest {
    /// Creates an empty request (for stub/fast path).
    #[inline]
    pub fn empty() -> Self {
        Self::default()
    }
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

impl ScriptResponse {
    /// Creates an empty response (for stub executor).
    #[inline]
    pub fn empty() -> Self {
        Self::default()
    }
}
