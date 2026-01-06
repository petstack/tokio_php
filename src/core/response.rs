//! HTTP response abstraction for middleware and executor.

use bytes::Bytes;
use http::header::{self, HeaderName};
use http::{HeaderMap, HeaderValue, StatusCode};

/// Common header name constants for fast lookup.
mod header_names {
    use super::*;
    pub static CONTENT_TYPE: HeaderName = header::CONTENT_TYPE;
    pub static RETRY_AFTER: HeaderName = header::RETRY_AFTER;
}

/// Pre-allocated static header values for common content types.
mod content_types {
    use super::*;
    pub static TEXT_PLAIN: HeaderValue = HeaderValue::from_static("text/plain; charset=utf-8");
    pub static TEXT_HTML: HeaderValue = HeaderValue::from_static("text/html; charset=utf-8");
    pub static APPLICATION_JSON: HeaderValue = HeaderValue::from_static("application/json");
}

/// Pre-allocated static bodies for common responses.
mod static_bodies {
    use super::*;
    pub static NOT_FOUND: Bytes = Bytes::from_static(b"Not Found");
    pub static SERVICE_UNAVAILABLE: Bytes = Bytes::from_static(b"Service Unavailable");
    pub static GATEWAY_TIMEOUT: Bytes = Bytes::from_static(b"Gateway Timeout");
    pub static TOO_MANY_REQUESTS: Bytes = Bytes::from_static(b"Too Many Requests");
}

/// HTTP response.
///
/// Note: Clone is intentionally not derived to prevent expensive copies.
/// Use references or move semantics instead.
#[derive(Debug)]
pub struct Response {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

impl Response {
    /// Create a new response builder.
    #[inline]
    pub fn builder() -> ResponseBuilder {
        ResponseBuilder::new()
    }

    /// Create a 200 OK response with body.
    #[inline]
    pub fn ok(body: impl Into<Bytes>) -> Self {
        Self {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: body.into(),
        }
    }

    /// Create a 404 Not Found response (uses static body).
    #[inline]
    pub fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            headers: HeaderMap::new(),
            body: static_bodies::NOT_FOUND.clone(), // Bytes::clone is cheap (Arc)
        }
    }

    /// Create a 500 Internal Server Error response.
    #[inline]
    pub fn internal_error(msg: &str) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            headers: HeaderMap::new(),
            body: Bytes::copy_from_slice(msg.as_bytes()),
        }
    }

    /// Create a 429 Too Many Requests response.
    #[inline]
    pub fn too_many_requests(retry_after: u64) -> Self {
        let mut headers = HeaderMap::with_capacity(1);
        if let Ok(value) = HeaderValue::try_from(retry_after.to_string()) {
            headers.insert(header_names::RETRY_AFTER.clone(), value);
        }
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            headers,
            body: static_bodies::TOO_MANY_REQUESTS.clone(),
        }
    }

    /// Create a 503 Service Unavailable response (uses static body).
    #[inline]
    pub fn service_unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            headers: HeaderMap::new(),
            body: static_bodies::SERVICE_UNAVAILABLE.clone(),
        }
    }

    /// Create a 504 Gateway Timeout response (uses static body).
    #[inline]
    pub fn gateway_timeout() -> Self {
        Self {
            status: StatusCode::GATEWAY_TIMEOUT,
            headers: HeaderMap::new(),
            body: static_bodies::GATEWAY_TIMEOUT.clone(),
        }
    }

    /// Create an empty response with given status.
    #[inline]
    pub fn empty(status: StatusCode) -> Self {
        Self {
            status,
            headers: HeaderMap::new(),
            body: Bytes::new(),
        }
    }

    // Getters

    /// Get the status code.
    #[inline]
    pub fn status(&self) -> StatusCode {
        self.status
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

    /// Get the response body.
    #[inline]
    pub fn body(&self) -> &Bytes {
        &self.body
    }

    /// Get a header value by HeaderName (fast path).
    #[inline]
    fn header_by_name(&self, name: &HeaderName) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// Get a header value by string name (slower, case-insensitive).
    #[inline]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    // Modifiers

    /// Set the status code.
    #[inline]
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Add a header (fast path with HeaderName + HeaderValue).
    #[inline]
    pub fn with_header_value(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }

    /// Add a header by string name and value.
    #[inline]
    pub fn with_header(mut self, name: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        if let (Ok(name), Ok(value)) = (
            HeaderName::try_from(name.as_ref()),
            HeaderValue::try_from(value.as_ref()),
        ) {
            self.headers.insert(name, value);
        }
        self
    }

    /// Set the body.
    #[inline]
    pub fn with_body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = body.into();
        self
    }

    // Status checks

    /// Check if this is a successful response (2xx).
    #[inline]
    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    /// Check if this is a client error (4xx).
    #[inline]
    pub fn is_client_error(&self) -> bool {
        self.status.is_client_error()
    }

    /// Check if this is a server error (5xx).
    #[inline]
    pub fn is_server_error(&self) -> bool {
        self.status.is_server_error()
    }

    /// Check if this is an error response (4xx or 5xx).
    #[inline]
    pub fn is_error(&self) -> bool {
        self.status.is_client_error() || self.status.is_server_error()
    }

    /// Get Content-Type header (fast path).
    #[inline]
    pub fn content_type(&self) -> Option<&str> {
        self.header_by_name(&header_names::CONTENT_TYPE)
    }

    /// Get body length.
    #[inline]
    pub fn body_len(&self) -> usize {
        self.body.len()
    }
}

impl Default for Response {
    fn default() -> Self {
        Self {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Bytes::new(),
        }
    }
}

impl From<Response> for http::Response<Bytes> {
    fn from(res: Response) -> Self {
        let mut builder = http::Response::builder().status(res.status);

        if let Some(headers) = builder.headers_mut() {
            *headers = res.headers;
        }

        builder.body(res.body).unwrap()
    }
}

impl<B> From<http::Response<B>> for Response
where
    B: Into<Bytes>,
{
    fn from(res: http::Response<B>) -> Self {
        let (parts, body) = res.into_parts();
        Self {
            status: parts.status,
            headers: parts.headers,
            body: body.into(),
        }
    }
}

/// Builder for creating HTTP responses.
pub struct ResponseBuilder {
    status: StatusCode,
    headers: Option<HeaderMap>, // Lazy allocation
    body: Bytes,
}

impl Default for ResponseBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseBuilder {
    /// Create a new response builder.
    #[inline]
    pub fn new() -> Self {
        Self {
            status: StatusCode::OK,
            headers: None, // Don't allocate until needed
            body: Bytes::new(),
        }
    }

    /// Set the status code.
    #[inline]
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Add header with typed HeaderName and HeaderValue (zero-alloc for static values).
    #[inline]
    pub fn header_value(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers
            .get_or_insert_with(HeaderMap::new)
            .insert(name, value);
        self
    }

    /// Add header by strings.
    #[inline]
    pub fn header(mut self, name: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        if let (Ok(name), Ok(value)) = (
            HeaderName::try_from(name.as_ref()),
            HeaderValue::try_from(value.as_ref()),
        ) {
            self.headers
                .get_or_insert_with(HeaderMap::new)
                .insert(name, value);
        }
        self
    }

    /// Set the body.
    #[inline]
    pub fn body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = body.into();
        self
    }

    /// Set Content-Type header (generic).
    #[inline]
    pub fn content_type(self, content_type: &str) -> Self {
        self.header("content-type", content_type)
    }

    /// Set Content-Type to text/html (uses static HeaderValue).
    #[inline]
    pub fn html(self) -> Self {
        self.header_value(
            header_names::CONTENT_TYPE.clone(),
            content_types::TEXT_HTML.clone(),
        )
    }

    /// Set Content-Type to application/json (uses static HeaderValue).
    #[inline]
    pub fn json(self) -> Self {
        self.header_value(
            header_names::CONTENT_TYPE.clone(),
            content_types::APPLICATION_JSON.clone(),
        )
    }

    /// Set Content-Type to text/plain (uses static HeaderValue).
    #[inline]
    pub fn text(self) -> Self {
        self.header_value(
            header_names::CONTENT_TYPE.clone(),
            content_types::TEXT_PLAIN.clone(),
        )
    }

    /// Build the response.
    #[inline]
    pub fn build(self) -> Response {
        Response {
            status: self.status,
            headers: self.headers.unwrap_or_default(),
            body: self.body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_builder() {
        let res = Response::builder()
            .status(StatusCode::CREATED)
            .header("x-custom", "value")
            .body("Hello")
            .build();

        assert_eq!(res.status(), StatusCode::CREATED);
        assert_eq!(res.header("x-custom"), Some("value"));
        assert_eq!(res.body().as_ref(), b"Hello");
    }

    #[test]
    fn test_response_ok() {
        let res = Response::ok("OK");
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.body().as_ref(), b"OK");
    }

    #[test]
    fn test_response_not_found() {
        let res = Response::not_found();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        assert!(res.is_client_error());
        assert!(res.is_error());
        assert_eq!(res.body().as_ref(), b"Not Found");
    }

    #[test]
    fn test_response_internal_error() {
        let res = Response::internal_error("Something went wrong");
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(res.is_server_error());
    }

    #[test]
    fn test_response_too_many_requests() {
        let res = Response::too_many_requests(60);
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(res.header("retry-after"), Some("60"));
    }

    #[test]
    fn test_response_service_unavailable() {
        let res = Response::service_unavailable();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(res.body().as_ref(), b"Service Unavailable");
    }

    #[test]
    fn test_response_gateway_timeout() {
        let res = Response::gateway_timeout();
        assert_eq!(res.status(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(res.body().as_ref(), b"Gateway Timeout");
    }

    #[test]
    fn test_response_with_modifiers() {
        let res = Response::ok("Original")
            .with_status(StatusCode::ACCEPTED)
            .with_header("x-test", "value")
            .with_body("Modified");

        assert_eq!(res.status(), StatusCode::ACCEPTED);
        assert_eq!(res.header("x-test"), Some("value"));
        assert_eq!(res.body().as_ref(), b"Modified");
    }

    #[test]
    fn test_response_content_types() {
        let html = Response::builder().html().body("<h1>Hi</h1>").build();
        assert_eq!(html.content_type(), Some("text/html; charset=utf-8"));

        let json = Response::builder().json().body("{}").build();
        assert_eq!(json.content_type(), Some("application/json"));

        let text = Response::builder().text().body("Hello").build();
        assert_eq!(text.content_type(), Some("text/plain; charset=utf-8"));
    }

    #[test]
    fn test_response_to_http() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .header("x-test", "value")
            .body("Hello")
            .build();

        let http_res: http::Response<Bytes> = res.into();
        assert_eq!(http_res.status(), StatusCode::OK);
        assert_eq!(http_res.headers().get("x-test").unwrap(), "value");
        assert_eq!(http_res.body().as_ref(), b"Hello");
    }

    #[test]
    fn test_response_empty_builder_no_headers() {
        // Builder should not allocate HeaderMap if no headers added
        let res = Response::builder().status(StatusCode::NO_CONTENT).build();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert!(res.headers().is_empty());
    }

    #[test]
    fn test_static_bodies_are_cheap_to_clone() {
        // Bytes::clone for static data is just Arc pointer copy
        let b1 = static_bodies::NOT_FOUND.clone();
        let b2 = static_bodies::NOT_FOUND.clone();
        assert_eq!(b1.as_ref(), b2.as_ref());
    }
}
