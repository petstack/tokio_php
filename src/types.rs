use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents an uploaded file from multipart form data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadedFile {
    pub name: String,
    pub mime_type: String,
    pub tmp_name: String,
    pub size: u64,
    pub error: u8,
}

/// Script execution request containing all HTTP request data.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScriptRequest {
    pub script_path: String,
    pub get_params: HashMap<String, String>,
    pub post_params: HashMap<String, String>,
    pub cookies: HashMap<String, String>,
    pub server_vars: HashMap<String, String>,
    pub files: HashMap<String, Vec<UploadedFile>>,
}

impl ScriptRequest {
    /// Creates an empty request (for stub/fast path).
    #[inline]
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Script execution response.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScriptResponse {
    pub body: String,
    pub headers: Vec<(String, String)>,
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
        }
    }

    /// Creates a response with body and headers.
    pub fn new(body: impl Into<String>, headers: Vec<(String, String)>) -> Self {
        Self {
            body: body.into(),
            headers,
        }
    }
}
