//! Per-request context for SAPI callbacks.
//!
//! This module provides thread-local storage for request-specific data
//! that is accessed by SAPI callbacks during PHP execution.

use std::cell::RefCell;
use std::ffi::{c_char, CString};
use std::path::PathBuf;

// Thread-local request context.
thread_local! {
    pub static REQUEST_CTX: RefCell<Option<RequestContext>> = const { RefCell::new(None) };
}

/// Per-request context data.
///
/// This structure holds all the request-specific data that SAPI callbacks
/// need to access during PHP execution.
pub struct RequestContext {
    /// $_SERVER variables (key, value pairs)
    server_vars: Vec<(String, String)>,

    /// Cookie string for read_cookies callback (pre-formatted)
    cookie_string: Option<CString>,

    /// POST body for php://input
    post_body: Option<Vec<u8>>,

    /// Current read position in post_body
    post_read_pos: usize,

    /// Virtual environment variables
    env_vars: std::collections::HashMap<String, CString>,

    /// Temporary files to clean up after request
    temp_files: Vec<PathBuf>,

    /// Request ID for tracing
    request_id: u64,

    /// Worker ID
    worker_id: u64,
}

impl RequestContext {
    /// Create a new request context.
    pub fn new(request_id: u64, worker_id: u64) -> Self {
        Self {
            server_vars: Vec::with_capacity(32),
            cookie_string: None,
            post_body: None,
            post_read_pos: 0,
            env_vars: std::collections::HashMap::new(),
            temp_files: Vec::new(),
            request_id,
            worker_id,
        }
    }

    /// Set $_SERVER variables.
    pub fn set_server_vars<I, K, V>(&mut self, vars: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        self.server_vars.clear();
        for (k, v) in vars {
            self.server_vars
                .push((k.as_ref().to_string(), v.as_ref().to_string()));
        }
    }

    /// Add a single $_SERVER variable.
    pub fn add_server_var(&mut self, key: &str, value: &str) {
        self.server_vars.push((key.to_string(), value.to_string()));
    }

    /// Get $_SERVER variables.
    pub fn server_vars(&self) -> &[(String, String)] {
        &self.server_vars
    }

    /// Set cookies from a list of (name, value) pairs.
    pub fn set_cookies<I, K, V>(&mut self, cookies: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let pairs: Vec<_> = cookies
            .into_iter()
            .map(|(k, v)| format!("{}={}", k.as_ref(), v.as_ref()))
            .collect();

        if pairs.is_empty() {
            self.cookie_string = None;
        } else {
            let cookie_str = pairs.join("; ");
            self.cookie_string = CString::new(cookie_str).ok();
        }
    }

    /// Get the cookie string for PHP's read_cookies callback.
    pub fn get_cookie_string(&self) -> *mut c_char {
        self.cookie_string
            .as_ref()
            .map(|s| s.as_ptr() as *mut c_char)
            .unwrap_or(std::ptr::null_mut())
    }

    /// Set the POST body for php://input.
    pub fn set_post_body(&mut self, body: Option<&[u8]>) {
        self.post_body = body.map(|b| b.to_vec());
        self.post_read_pos = 0;
    }

    /// Read POST data into buffer.
    ///
    /// This is called by the read_post SAPI callback.
    pub fn read_post(&mut self, buffer: *mut c_char, count_bytes: usize) -> usize {
        if let Some(ref body) = self.post_body {
            let remaining = body.len().saturating_sub(self.post_read_pos);
            let to_read = remaining.min(count_bytes);

            if to_read > 0 {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        body.as_ptr().add(self.post_read_pos),
                        buffer.cast::<u8>(),
                        to_read,
                    );
                }
                self.post_read_pos += to_read;
            }
            return to_read;
        }
        0
    }

    /// Set a virtual environment variable.
    pub fn set_env(&mut self, name: &str, value: &str) {
        if let Ok(cstring) = CString::new(value) {
            self.env_vars.insert(name.to_string(), cstring);
        }
    }

    /// Get a virtual environment variable.
    pub fn get_env(&self, name: &str) -> Option<&CString> {
        self.env_vars.get(name)
    }

    /// Register a temporary file for cleanup.
    pub fn register_temp_file(&mut self, path: PathBuf) {
        self.temp_files.push(path);
    }

    /// Get request ID.
    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    /// Get worker ID.
    pub fn worker_id(&self) -> u64 {
        self.worker_id
    }

    /// Clean up temporary files.
    pub fn cleanup(&mut self) {
        for path in self.temp_files.drain(..) {
            if let Err(e) = std::fs::remove_file(&path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "Failed to cleanup temp file"
                    );
                }
            }
        }
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

impl Drop for RequestContext {
    fn drop(&mut self) {
        self.cleanup();
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Initialize request context for the current thread.
pub fn init_context(
    request_id: u64,
    worker_id: u64,
) -> &'static std::thread::LocalKey<RefCell<Option<RequestContext>>> {
    REQUEST_CTX.with(|ctx| {
        *ctx.borrow_mut() = Some(RequestContext::new(request_id, worker_id));
    });
    &REQUEST_CTX
}

/// Set request data (server vars, cookies, post body).
pub fn set_request_data<S, C>(server_vars: S, cookies: C, post_body: Option<&[u8]>)
where
    S: IntoIterator<Item = (String, String)>,
    C: IntoIterator<Item = (String, String)>,
{
    REQUEST_CTX.with(|ctx| {
        if let Some(ref mut context) = *ctx.borrow_mut() {
            context.set_server_vars(server_vars);
            context.set_cookies(cookies);
            context.set_post_body(post_body);
        }
    });
}

/// Clear request context.
pub fn clear_context() {
    REQUEST_CTX.with(|ctx| {
        if let Some(ref mut context) = ctx.borrow_mut().take() {
            context.cleanup();
        }
    });
}

/// Execute a closure with access to the request context.
pub fn with_context<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&RequestContext) -> R,
{
    REQUEST_CTX.with(|ctx| ctx.borrow().as_ref().map(f))
}

/// Execute a closure with mutable access to the request context.
pub fn with_context_mut<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut RequestContext) -> R,
{
    REQUEST_CTX.with(|ctx| ctx.borrow_mut().as_mut().map(f))
}

/// Register a temporary file for cleanup after request completes.
pub fn register_temp_file(path: PathBuf) {
    with_context_mut(|ctx| ctx.register_temp_file(path));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_context_creation() {
        let ctx = RequestContext::new(123, 5);
        assert_eq!(ctx.request_id(), 123);
        assert_eq!(ctx.worker_id(), 5);
    }

    #[test]
    fn test_server_vars() {
        let mut ctx = RequestContext::new(1, 1);
        ctx.set_server_vars([
            ("REQUEST_METHOD".to_string(), "GET".to_string()),
            ("REQUEST_URI".to_string(), "/test".to_string()),
        ]);

        assert_eq!(ctx.server_vars().len(), 2);
        assert_eq!(
            ctx.server_vars()[0],
            ("REQUEST_METHOD".to_string(), "GET".to_string())
        );
    }

    #[test]
    fn test_cookies() {
        let mut ctx = RequestContext::new(1, 1);
        ctx.set_cookies([
            ("session".to_string(), "abc123".to_string()),
            ("user".to_string(), "test".to_string()),
        ]);

        let cookie_str = ctx.get_cookie_string();
        assert!(!cookie_str.is_null());

        unsafe {
            let s = std::ffi::CStr::from_ptr(cookie_str).to_str().unwrap();
            assert!(s.contains("session=abc123"));
            assert!(s.contains("user=test"));
        }
    }

    #[test]
    fn test_post_body() {
        let mut ctx = RequestContext::new(1, 1);
        ctx.set_post_body(Some(b"test=value"));

        let mut buffer = [0u8; 100];
        let read = ctx.read_post(buffer.as_mut_ptr() as *mut c_char, 100);
        assert_eq!(read, 10);
        assert_eq!(&buffer[..read], b"test=value");

        // Second read should return 0
        let read = ctx.read_post(buffer.as_mut_ptr() as *mut c_char, 100);
        assert_eq!(read, 0);
    }

    #[test]
    fn test_env_vars() {
        let mut ctx = RequestContext::new(1, 1);
        ctx.set_env("MY_VAR", "my_value");

        let val = ctx.get_env("MY_VAR");
        assert!(val.is_some());
        assert_eq!(val.unwrap().to_str().unwrap(), "my_value");

        let missing = ctx.get_env("MISSING");
        assert!(missing.is_none());
    }
}
