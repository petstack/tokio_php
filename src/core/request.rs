//! HTTP request abstraction for middleware and executor.

use bytes::Bytes;
use http::header::{self, HeaderName};
use http::{HeaderMap, Method, Uri};

/// Header name constants for fast lookup.
mod header_names {
    use super::*;

    pub static ACCEPT: HeaderName = header::ACCEPT;
    pub static ACCEPT_ENCODING: HeaderName = header::ACCEPT_ENCODING;
    pub static CONTENT_TYPE: HeaderName = header::CONTENT_TYPE;
    pub static CONTENT_LENGTH: HeaderName = header::CONTENT_LENGTH;
    pub static USER_AGENT: HeaderName = header::USER_AGENT;
}

/// Lazily initialized custom header names.
static X_REQUEST_ID: std::sync::LazyLock<HeaderName> =
    std::sync::LazyLock::new(|| HeaderName::from_static("x-request-id"));
static X_PROFILE: std::sync::LazyLock<HeaderName> =
    std::sync::LazyLock::new(|| HeaderName::from_static("x-profile"));

/// HTTP request for middleware and executor.
///
/// Note: Clone is intentionally not derived to prevent expensive copies.
/// Use references or move semantics instead.
#[derive(Debug)]
pub struct Request {
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
    version: http::Version,
}

impl Request {
    /// Create a new request.
    #[inline]
    pub fn new(method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Self {
        Self {
            method,
            uri,
            headers,
            body,
            version: http::Version::HTTP_11,
        }
    }

    /// Get the HTTP method.
    #[inline]
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Get the request path.
    #[inline]
    pub fn path(&self) -> &str {
        self.uri.path()
    }

    /// Get the query string.
    #[inline]
    pub fn query(&self) -> Option<&str> {
        self.uri.query()
    }

    /// Get the full URI.
    #[inline]
    pub fn uri(&self) -> &Uri {
        &self.uri
    }

    /// Get the headers.
    #[inline]
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get a mutable reference to headers.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }

    /// Get the request body.
    #[inline]
    pub fn body(&self) -> &Bytes {
        &self.body
    }

    /// Get the HTTP version.
    #[inline]
    pub fn version(&self) -> http::Version {
        self.version
    }

    /// Set the HTTP version.
    #[inline]
    pub fn set_version(&mut self, version: http::Version) {
        self.version = version;
    }

    /// Get a header value by name (fast path with HeaderName constant).
    #[inline]
    fn header_by_name(&self, name: &HeaderName) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// Get a header value by string name (slower, case-insensitive).
    #[inline]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// Get Accept header value.
    #[inline]
    pub fn accept(&self) -> Option<&str> {
        self.header_by_name(&header_names::ACCEPT)
    }

    /// Get Accept-Encoding header value.
    #[inline]
    pub fn accept_encoding(&self) -> Option<&str> {
        self.header_by_name(&header_names::ACCEPT_ENCODING)
    }

    /// Check if client accepts HTML responses.
    /// Handles wildcards: text/html, text/*, */*
    #[inline]
    pub fn accepts_html(&self) -> bool {
        self.accept()
            .map(|v| v.contains("text/html") || v.contains("*/*") || v.contains("text/*"))
            .unwrap_or(false)
    }

    /// Check if client accepts Brotli compression.
    #[inline]
    pub fn accepts_brotli(&self) -> bool {
        self.accept_encoding()
            .map(|v| v.contains("br"))
            .unwrap_or(false)
    }

    /// Check if client accepts Gzip compression.
    #[inline]
    pub fn accepts_gzip(&self) -> bool {
        self.accept_encoding()
            .map(|v| v.contains("gzip"))
            .unwrap_or(false)
    }

    /// Get Content-Type header.
    #[inline]
    pub fn content_type(&self) -> Option<&str> {
        self.header_by_name(&header_names::CONTENT_TYPE)
    }

    /// Get Content-Length header.
    #[inline]
    pub fn content_length(&self) -> Option<u64> {
        self.header_by_name(&header_names::CONTENT_LENGTH)
            .and_then(|v| v.parse().ok())
    }

    /// Get User-Agent header.
    #[inline]
    pub fn user_agent(&self) -> Option<&str> {
        self.header_by_name(&header_names::USER_AGENT)
    }

    /// Get X-Request-ID header.
    #[inline]
    pub fn request_id(&self) -> Option<&str> {
        self.header_by_name(&X_REQUEST_ID)
    }

    /// Check if this is a profiling request.
    #[inline]
    pub fn is_profiling(&self) -> bool {
        self.header_by_name(&X_PROFILE) == Some("1")
    }
}

impl<B> From<http::Request<B>> for Request
where
    B: Into<Bytes>,
{
    fn from(req: http::Request<B>) -> Self {
        let (parts, body) = req.into_parts();
        Self {
            method: parts.method,
            uri: parts.uri,
            headers: parts.headers,
            body: body.into(),
            version: parts.version,
        }
    }
}

impl From<Request> for http::Request<Bytes> {
    fn from(req: Request) -> Self {
        let mut builder = http::Request::builder()
            .method(req.method)
            .uri(req.uri)
            .version(req.version);

        if let Some(headers) = builder.headers_mut() {
            *headers = req.headers;
        }

        builder.body(req.body).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_from_http() {
        let http_req = http::Request::builder()
            .method("GET")
            .uri("/test?foo=bar")
            .header("accept", "text/html")
            .header("accept-encoding", "br, gzip")
            .body(Bytes::new())
            .unwrap();

        let req = Request::from(http_req);

        assert_eq!(req.method(), Method::GET);
        assert_eq!(req.path(), "/test");
        assert_eq!(req.query(), Some("foo=bar"));
        assert!(req.accepts_html());
        assert!(req.accepts_brotli());
        assert!(req.accepts_gzip());
    }

    #[test]
    fn test_request_headers() {
        let http_req = http::Request::builder()
            .method("POST")
            .uri("/api")
            .header("content-type", "application/json")
            .header("content-length", "42")
            .header("user-agent", "test/1.0")
            .header("x-request-id", "abc123")
            .header("x-profile", "1")
            .body(Bytes::new())
            .unwrap();

        let req = Request::from(http_req);

        assert_eq!(req.content_type(), Some("application/json"));
        assert_eq!(req.content_length(), Some(42));
        assert_eq!(req.user_agent(), Some("test/1.0"));
        assert_eq!(req.request_id(), Some("abc123"));
        assert!(req.is_profiling());
    }

    #[test]
    fn test_accepts_html_wildcards() {
        // Test */* wildcard
        let req = http::Request::builder()
            .method("GET")
            .uri("/")
            .header("accept", "*/*")
            .body(Bytes::new())
            .unwrap();
        assert!(Request::from(req).accepts_html());

        // Test text/* wildcard
        let req = http::Request::builder()
            .method("GET")
            .uri("/")
            .header("accept", "text/*")
            .body(Bytes::new())
            .unwrap();
        assert!(Request::from(req).accepts_html());

        // Test application/json (should not accept HTML)
        let req = http::Request::builder()
            .method("GET")
            .uri("/")
            .header("accept", "application/json")
            .body(Bytes::new())
            .unwrap();
        assert!(!Request::from(req).accepts_html());
    }

    #[test]
    fn test_header_by_string() {
        let http_req = http::Request::builder()
            .method("GET")
            .uri("/")
            .header("x-custom-header", "custom-value")
            .body(Bytes::new())
            .unwrap();

        let req = Request::from(http_req);
        assert_eq!(req.header("x-custom-header"), Some("custom-value"));
        assert_eq!(req.header("X-Custom-Header"), Some("custom-value")); // case-insensitive
    }
}
