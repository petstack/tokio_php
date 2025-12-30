//! HTTP request abstraction for middleware and executor.

use bytes::Bytes;
use http::{HeaderMap, Method, Uri};

/// HTTP request for middleware and executor.
#[derive(Clone, Debug)]
pub struct Request {
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
    version: http::Version,
}

impl Request {
    /// Create a new request.
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
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Get the request path.
    pub fn path(&self) -> &str {
        self.uri.path()
    }

    /// Get the query string.
    pub fn query(&self) -> Option<&str> {
        self.uri.query()
    }

    /// Get the full URI.
    pub fn uri(&self) -> &Uri {
        &self.uri
    }

    /// Get the headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get a mutable reference to headers.
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }

    /// Get the request body.
    pub fn body(&self) -> &Bytes {
        &self.body
    }

    /// Get the HTTP version.
    pub fn version(&self) -> http::Version {
        self.version
    }

    /// Set the HTTP version.
    pub fn set_version(&mut self, version: http::Version) {
        self.version = version;
    }

    /// Get a header value by name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// Check if client accepts HTML responses.
    pub fn accepts_html(&self) -> bool {
        self.header("accept")
            .map(|v| v.contains("text/html"))
            .unwrap_or(false)
    }

    /// Check if client accepts Brotli compression.
    pub fn accepts_brotli(&self) -> bool {
        self.header("accept-encoding")
            .map(|v| v.contains("br"))
            .unwrap_or(false)
    }

    /// Check if client accepts Gzip compression.
    pub fn accepts_gzip(&self) -> bool {
        self.header("accept-encoding")
            .map(|v| v.contains("gzip"))
            .unwrap_or(false)
    }

    /// Get Content-Type header.
    pub fn content_type(&self) -> Option<&str> {
        self.header("content-type")
    }

    /// Get Content-Length header.
    pub fn content_length(&self) -> Option<u64> {
        self.header("content-length")
            .and_then(|v| v.parse().ok())
    }

    /// Get User-Agent header.
    pub fn user_agent(&self) -> Option<&str> {
        self.header("user-agent")
    }

    /// Get X-Request-ID header.
    pub fn request_id(&self) -> Option<&str> {
        self.header("x-request-id")
    }

    /// Check if this is a profiling request.
    pub fn is_profiling(&self) -> bool {
        self.header("x-profile") == Some("1")
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
}
