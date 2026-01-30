//! Request tracing middleware for OpenTelemetry.
//!
//! Provides automatic span creation and context propagation for
//! HTTP requests and PHP script execution.

use http::{HeaderMap, Request, Response};
use opentelemetry::{
    global,
    propagation::{Extractor, Injector, TextMapPropagator},
    trace::{SpanKind, Status, TraceContextExt, Tracer},
    Context, KeyValue,
};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_semantic_conventions::trace::{
    HTTP_REQUEST_METHOD, HTTP_RESPONSE_STATUS_CODE, HTTP_ROUTE, NETWORK_PROTOCOL_VERSION, URL_PATH,
    URL_QUERY,
};
use std::time::Instant;

/// Extract W3C Trace Context from incoming request headers.
pub fn extract_context<B>(request: &Request<B>) -> Context {
    let propagator = TraceContextPropagator::new();
    let extractor = HeaderExtractor(request.headers());
    propagator.extract(&extractor)
}

/// Extract context from header map directly.
pub fn extract_context_from_headers(headers: &HeaderMap) -> Context {
    let propagator = TraceContextPropagator::new();
    let extractor = HeaderExtractor(headers);
    propagator.extract(&extractor)
}

/// Inject W3C Trace Context into response headers.
pub fn inject_context<B>(response: &mut Response<B>, context: &Context) {
    let propagator = TraceContextPropagator::new();
    let mut injector = HeaderInjector(response.headers_mut());
    propagator.inject_context(context, &mut injector);
}

/// Inject context into header map directly.
pub fn inject_context_into_headers(headers: &mut HeaderMap, context: &Context) {
    let propagator = TraceContextPropagator::new();
    let mut injector = HeaderInjector(headers);
    propagator.inject_context(context, &mut injector);
}

/// Create a server span for an HTTP request.
///
/// Returns the span context for use in nested spans.
pub fn start_http_span<B>(request: &Request<B>, parent_context: &Context) -> Context {
    let tracer = global::tracer("tokio_php");

    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let query = request.uri().query().unwrap_or("").to_string();
    let version = format!("{:?}", request.version());

    let span = tracer
        .span_builder(format!("{} {}", method, path))
        .with_kind(SpanKind::Server)
        .with_attributes(vec![
            KeyValue::new(HTTP_REQUEST_METHOD, method),
            KeyValue::new(URL_PATH, path.clone()),
            KeyValue::new(URL_QUERY, query),
            KeyValue::new(NETWORK_PROTOCOL_VERSION, version),
            KeyValue::new(HTTP_ROUTE, path),
        ])
        .start_with_context(&tracer, parent_context);

    Context::current_with_span(span)
}

/// End an HTTP span with response information.
pub fn end_http_span(context: &Context, status_code: u16, duration_ms: f64) {
    let span = context.span();

    span.set_attribute(KeyValue::new(HTTP_RESPONSE_STATUS_CODE, status_code as i64));
    span.set_attribute(KeyValue::new("http.request.duration_ms", duration_ms));

    // Set span status based on HTTP status
    if status_code >= 500 {
        span.set_status(Status::error(format!("HTTP {}", status_code)));
    } else if status_code >= 400 {
        // Client errors are not span errors per OpenTelemetry spec
        span.set_status(Status::Ok);
    } else {
        span.set_status(Status::Ok);
    }

    span.end();
}

/// Create a span for PHP script execution.
pub fn start_php_span(script_path: &str, parent_context: &Context) -> Context {
    let tracer = global::tracer("tokio_php");

    let span = tracer
        .span_builder("PHP execute")
        .with_kind(SpanKind::Internal)
        .with_attributes(vec![KeyValue::new("php.script", script_path.to_string())])
        .start_with_context(&tracer, parent_context);

    Context::current_with_span(span)
}

/// Record PHP execution metrics in the current span.
pub fn record_php_metrics(
    context: &Context,
    startup_us: u64,
    exec_us: u64,
    shutdown_us: u64,
    superglobals_us: u64,
) {
    let span = context.span();
    span.set_attribute(KeyValue::new("php.startup_us", startup_us as i64));
    span.set_attribute(KeyValue::new("php.exec_us", exec_us as i64));
    span.set_attribute(KeyValue::new("php.shutdown_us", shutdown_us as i64));
    span.set_attribute(KeyValue::new("php.superglobals_us", superglobals_us as i64));
    span.set_attribute(KeyValue::new(
        "php.total_us",
        (startup_us + exec_us + shutdown_us + superglobals_us) as i64,
    ));
}

/// End a PHP execution span.
pub fn end_php_span(context: &Context, success: bool, output_size: usize) {
    let span = context.span();

    span.set_attribute(KeyValue::new("php.output_size", output_size as i64));

    if success {
        span.set_status(Status::Ok);
    } else {
        span.set_status(Status::error("PHP execution failed"));
    }

    span.end();
}

/// Create a span for queue wait time.
pub fn start_queue_span(parent_context: &Context) -> Context {
    let tracer = global::tracer("tokio_php");

    let span = tracer
        .span_builder("queue wait")
        .with_kind(SpanKind::Internal)
        .start_with_context(&tracer, parent_context);

    Context::current_with_span(span)
}

/// End queue wait span with duration.
pub fn end_queue_span(context: &Context, wait_us: u64) {
    let span = context.span();
    span.set_attribute(KeyValue::new("queue.wait_us", wait_us as i64));
    span.set_status(Status::Ok);
    span.end();
}

/// Helper struct for tracing an HTTP request with timing.
pub struct TracedRequest {
    context: Context,
    start: Instant,
}

impl TracedRequest {
    /// Start tracing a new HTTP request.
    pub fn new<B>(request: &Request<B>) -> Self {
        let parent = extract_context(request);
        let context = start_http_span(request, &parent);

        Self {
            context,
            start: Instant::now(),
        }
    }

    /// Get the trace context for propagation.
    pub fn context(&self) -> &Context {
        &self.context
    }

    /// End the request trace with response info.
    pub fn end(self, status_code: u16) {
        let duration_ms = self.start.elapsed().as_secs_f64() * 1000.0;
        end_http_span(&self.context, status_code, duration_ms);
    }
}

// Header extractor for OpenTelemetry propagation
struct HeaderExtractor<'a>(&'a HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}

// Header injector for OpenTelemetry propagation
struct HeaderInjector<'a>(&'a mut HeaderMap);

impl Injector for HeaderInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let Ok(name) = http::header::HeaderName::from_bytes(key.as_bytes()) {
            if let Ok(val) = http::header::HeaderValue::from_str(&value) {
                self.0.insert(name, val);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Request;

    #[test]
    fn test_extract_context_no_header() {
        let request = Request::builder().uri("/test").body(()).unwrap();

        let context = extract_context(&request);
        // Should return a valid context even without traceparent header
        assert!(!context.has_active_span());
    }

    #[test]
    fn test_header_extractor() {
        let mut headers = HeaderMap::new();
        headers.insert("traceparent", "00-1234-5678-01".parse().unwrap());

        let extractor = HeaderExtractor(&headers);
        assert_eq!(extractor.get("traceparent"), Some("00-1234-5678-01"));
        assert_eq!(extractor.get("missing"), None);
    }
}
