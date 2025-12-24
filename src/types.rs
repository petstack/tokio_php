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

impl UploadedFile {
    /// Check if the upload was successful.
    #[inline]
    pub fn is_ok(&self) -> bool {
        self.error == 0
    }
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

    /// Creates a new builder for ScriptRequest.
    #[inline]
    pub fn builder() -> ScriptRequestBuilder {
        ScriptRequestBuilder::new()
    }
}

/// Builder for constructing ScriptRequest instances.
#[derive(Debug, Default)]
pub struct ScriptRequestBuilder {
    script_path: String,
    get_params: ParamList,
    post_params: ParamList,
    cookies: ParamList,
    server_vars: ParamList,
    files: Vec<(String, Vec<UploadedFile>)>,
    profile: bool,
}

impl ScriptRequestBuilder {
    /// Creates a new builder.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the script path.
    #[inline]
    pub fn script_path(mut self, path: impl Into<String>) -> Self {
        self.script_path = path.into();
        self
    }

    /// Sets GET parameters.
    #[inline]
    pub fn get_params(mut self, params: ParamList) -> Self {
        self.get_params = params;
        self
    }

    /// Adds a single GET parameter.
    #[inline]
    pub fn get_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.get_params.push((key.into(), value.into()));
        self
    }

    /// Sets POST parameters.
    #[inline]
    pub fn post_params(mut self, params: ParamList) -> Self {
        self.post_params = params;
        self
    }

    /// Adds a single POST parameter.
    #[inline]
    pub fn post_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.post_params.push((key.into(), value.into()));
        self
    }

    /// Sets cookies.
    #[inline]
    pub fn cookies(mut self, cookies: ParamList) -> Self {
        self.cookies = cookies;
        self
    }

    /// Adds a single cookie.
    #[inline]
    pub fn cookie(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.cookies.push((name.into(), value.into()));
        self
    }

    /// Sets server variables.
    #[inline]
    pub fn server_vars(mut self, vars: ParamList) -> Self {
        self.server_vars = vars;
        self
    }

    /// Adds a single server variable.
    #[inline]
    pub fn server_var(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.server_vars.push((name.into(), value.into()));
        self
    }

    /// Sets uploaded files.
    #[inline]
    pub fn files(mut self, files: Vec<(String, Vec<UploadedFile>)>) -> Self {
        self.files = files;
        self
    }

    /// Enables or disables profiling.
    #[inline]
    pub fn profile(mut self, enabled: bool) -> Self {
        self.profile = enabled;
        self
    }

    /// Builds the ScriptRequest.
    #[inline]
    pub fn build(self) -> ScriptRequest {
        ScriptRequest {
            script_path: self.script_path,
            get_params: self.get_params,
            post_params: self.post_params,
            cookies: self.cookies,
            server_vars: self.server_vars,
            files: self.files,
            profile: self.profile,
        }
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

    /// Creates a response with just a body.
    #[inline]
    pub fn with_body(body: impl Into<String>) -> Self {
        Self {
            body: body.into(),
            headers: Vec::new(),
            profile: None,
        }
    }

    /// Creates a response with body and headers.
    #[inline]
    pub fn new(body: impl Into<String>, headers: Vec<(String, String)>) -> Self {
        Self {
            body: body.into(),
            headers,
            profile: None,
        }
    }

    /// Creates a new builder for ScriptResponse.
    #[inline]
    pub fn builder() -> ScriptResponseBuilder {
        ScriptResponseBuilder::new()
    }
}

/// Builder for constructing ScriptResponse instances.
#[derive(Debug, Default)]
pub struct ScriptResponseBuilder {
    body: String,
    headers: Vec<(String, String)>,
    profile: Option<ProfileData>,
}

impl ScriptResponseBuilder {
    /// Creates a new builder.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the response body.
    #[inline]
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    /// Sets the response headers.
    #[inline]
    pub fn headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers = headers;
        self
    }

    /// Adds a single header.
    #[inline]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Sets the profile data.
    #[inline]
    pub fn profile(mut self, profile: ProfileData) -> Self {
        self.profile = Some(profile);
        self
    }

    /// Builds the ScriptResponse.
    #[inline]
    pub fn build(self) -> ScriptResponse {
        ScriptResponse {
            body: self.body,
            headers: self.headers,
            profile: self.profile,
        }
    }
}

// =============================================================================
// Error Types
// =============================================================================

/// Errors that can occur during request parsing.
#[derive(Debug, Clone)]
pub enum ParseError {
    /// Missing boundary in multipart content-type
    InvalidBoundary,
    /// File exceeds maximum upload size
    FileTooLarge(u64),
    /// Failed to read request body
    BodyReadFailed(String),
    /// Invalid query string encoding
    InvalidEncoding(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::InvalidBoundary => write!(f, "Missing boundary in multipart content-type"),
            ParseError::FileTooLarge(size) => write!(f, "File too large: {} bytes", size),
            ParseError::BodyReadFailed(msg) => write!(f, "Failed to read body: {}", msg),
            ParseError::InvalidEncoding(msg) => write!(f, "Invalid encoding: {}", msg),
        }
    }
}

impl std::error::Error for ParseError {}
