use crate::profiler::ProfileData;

/// Key-value pair type for parameters (faster than HashMap for small collections)
pub type ParamList = Vec<(String, String)>;

/// Represents an uploaded file from multipart form data.
#[derive(Debug, Clone)]
pub struct UploadedFile {
    pub name: String,
    pub mime_type: String,
    pub tmp_name: String,
    pub size: u64,
    pub error: u8,
}

/// Script execution request containing all HTTP request data.
#[derive(Debug, Clone, Default)]
pub struct ScriptRequest {
    pub script_path: String,
    pub get_params: ParamList,
    pub post_params: ParamList,
    pub cookies: ParamList,
    pub server_vars: ParamList,
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

/// Script execution response.
#[derive(Debug, Clone, Default)]
pub struct ScriptResponse {
    pub body: String,
    pub headers: Vec<(String, String)>,
    /// Profiling data (if profiling was enabled)
    pub profile: Option<ProfileData>,
}

impl ScriptResponse {
    /// Creates an empty response (for stub executor).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Creates a response with just a body.
    pub fn with_body(body: impl Into<String>) -> Self {
        Self {
            body: body.into(),
            headers: Vec::new(),
            profile: None,
        }
    }

    /// Creates a response with body and headers.
    pub fn new(body: impl Into<String>, headers: Vec<(String, String)>) -> Self {
        Self {
            body: body.into(),
            headers,
            profile: None,
        }
    }
}
