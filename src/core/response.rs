//! HTTP response abstraction for middleware and executor.

use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode};

/// HTTP response.
#[derive(Clone, Debug)]
pub struct Response {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

impl Response {
    /// Create a new response builder.
    pub fn builder() -> ResponseBuilder {
        ResponseBuilder::new()
    }

    /// Create a 200 OK response with body.
    pub fn ok(body: impl Into<Bytes>) -> Self {
        Self::builder().status(StatusCode::OK).body(body).build()
    }

    /// Create a 404 Not Found response.
    pub fn not_found() -> Self {
        Self::builder()
            .status(StatusCode::NOT_FOUND)
            .body("Not Found")
            .build()
    }

    /// Create a 500 Internal Server Error response.
    pub fn internal_error(msg: &str) -> Self {
        Self::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(msg.to_string())
            .build()
    }

    /// Create a 429 Too Many Requests response.
    pub fn too_many_requests(retry_after: u64) -> Self {
        Self::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header("Retry-After", retry_after.to_string())
            .body("Too Many Requests")
            .build()
    }

    /// Create a 503 Service Unavailable response.
    pub fn service_unavailable() -> Self {
        Self::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body("Service Unavailable")
            .build()
    }

    /// Create a 504 Gateway Timeout response.
    pub fn gateway_timeout() -> Self {
        Self::builder()
            .status(StatusCode::GATEWAY_TIMEOUT)
            .body("Gateway Timeout")
            .build()
    }

    /// Create an empty response with given status.
    pub fn empty(status: StatusCode) -> Self {
        Self::builder().status(status).build()
    }

    // Getters

    /// Get the status code.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Get the headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get a mutable reference to headers.
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }

    /// Get the response body.
    pub fn body(&self) -> &Bytes {
        &self.body
    }

    /// Get a header value by name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    // Modifiers (return new Response for chaining)

    /// Set the status code.
    pub fn with_status(mut self, status: impl Into<StatusCode>) -> Self {
        self.status = status.into();
        self
    }

    /// Add a header.
    pub fn with_header(mut self, name: impl AsRef<str>, value: impl ToString) -> Self {
        if let (Ok(name), Ok(value)) = (
            HeaderName::try_from(name.as_ref()),
            HeaderValue::try_from(value.to_string()),
        ) {
            self.headers.insert(name, value);
        }
        self
    }

    /// Set the body.
    pub fn with_body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = body.into();
        self
    }

    /// Check if this is a successful response (2xx).
    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    /// Check if this is a client error (4xx).
    pub fn is_client_error(&self) -> bool {
        self.status.is_client_error()
    }

    /// Check if this is a server error (5xx).
    pub fn is_server_error(&self) -> bool {
        self.status.is_server_error()
    }

    /// Check if this is an error response (4xx or 5xx).
    pub fn is_error(&self) -> bool {
        self.is_client_error() || self.is_server_error()
    }

    /// Get Content-Type header.
    pub fn content_type(&self) -> Option<&str> {
        self.header("content-type")
    }

    /// Get body length.
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
#[derive(Default)]
pub struct ResponseBuilder {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

impl ResponseBuilder {
    /// Create a new response builder.
    pub fn new() -> Self {
        Self {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Bytes::new(),
        }
    }

    /// Set the status code.
    pub fn status(mut self, status: impl Into<StatusCode>) -> Self {
        self.status = status.into();
        self
    }

    /// Add a header.
    pub fn header(mut self, name: impl AsRef<str>, value: impl ToString) -> Self {
        if let (Ok(name), Ok(value)) = (
            HeaderName::try_from(name.as_ref()),
            HeaderValue::try_from(value.to_string()),
        ) {
            self.headers.insert(name, value);
        }
        self
    }

    /// Set the body.
    pub fn body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = body.into();
        self
    }

    /// Set Content-Type header.
    pub fn content_type(self, content_type: &str) -> Self {
        self.header("content-type", content_type)
    }

    /// Set Content-Type to text/html.
    pub fn html(self) -> Self {
        self.content_type("text/html; charset=utf-8")
    }

    /// Set Content-Type to application/json.
    pub fn json(self) -> Self {
        self.content_type("application/json")
    }

    /// Set Content-Type to text/plain.
    pub fn text(self) -> Self {
        self.content_type("text/plain; charset=utf-8")
    }

    /// Build the response.
    pub fn build(self) -> Response {
        Response {
            status: self.status,
            headers: self.headers,
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
}
