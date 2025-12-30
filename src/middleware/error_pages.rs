//! Custom error pages middleware.
//!
//! Replaces error responses (4xx/5xx) with custom HTML pages when:
//! - Client accepts text/html
//! - Response body is empty
//! - A custom page exists for the status code

use bytes::Bytes;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::core::{Context, Request, Response};

use super::{Middleware, MiddlewareResult};

/// Cache of custom error pages loaded at startup.
#[derive(Clone)]
pub struct ErrorPages {
    pages: Arc<HashMap<u16, Bytes>>,
}

impl ErrorPages {
    /// Create an empty error pages cache.
    pub fn new() -> Self {
        Self {
            pages: Arc::new(HashMap::new()),
        }
    }

    /// Load error pages from a directory.
    ///
    /// Scans for files matching `{status_code}.html` pattern.
    pub fn from_directory(dir: impl AsRef<Path>) -> Self {
        let path = dir.as_ref();

        if !path.exists() || !path.is_dir() {
            tracing::warn!("Error pages directory not found: {}", path.display());
            return Self::new();
        }

        let mut pages = HashMap::new();

        let entries = match std::fs::read_dir(path) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!("Failed to read error pages directory: {}", e);
                return Self::new();
            }
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let file_path = entry.path();

            // Check if it's an HTML file
            if file_path.extension().and_then(|e| e.to_str()) != Some("html") {
                continue;
            }

            // Extract status code from filename
            let file_stem = match file_path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };

            let status_code: u16 = match file_stem.parse() {
                Ok(code) if (400..600).contains(&code) => code,
                _ => continue,
            };

            // Read file content
            match std::fs::read(&file_path) {
                Ok(content) => {
                    tracing::debug!(
                        "Loaded error page: {} ({} bytes)",
                        file_path.display(),
                        content.len()
                    );
                    pages.insert(status_code, Bytes::from(content));
                }
                Err(e) => {
                    tracing::warn!("Failed to read error page {}: {}", file_path.display(), e);
                }
            }
        }

        if !pages.is_empty() {
            let codes: Vec<_> = pages.keys().collect();
            tracing::info!("Loaded {} error pages: {:?}", pages.len(), codes);
        }

        Self {
            pages: Arc::new(pages),
        }
    }

    /// Get the HTML content for a status code.
    pub fn get(&self, status_code: u16) -> Option<&Bytes> {
        self.pages.get(&status_code)
    }

    /// Check if any error pages are configured.
    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// Get the number of loaded error pages.
    pub fn len(&self) -> usize {
        self.pages.len()
    }
}

impl Default for ErrorPages {
    fn default() -> Self {
        Self::new()
    }
}

/// Custom error pages middleware.
///
/// Replaces empty error responses with custom HTML pages.
pub struct ErrorPagesMiddleware {
    pages: ErrorPages,
}

impl ErrorPagesMiddleware {
    /// Create from loaded error pages.
    pub fn new(pages: ErrorPages) -> Self {
        Self { pages }
    }

    /// Create from directory path.
    pub fn from_directory(dir: impl AsRef<Path>) -> Self {
        Self {
            pages: ErrorPages::from_directory(dir),
        }
    }

    /// Create from optional directory path.
    /// Returns None if path is None or no pages are loaded.
    pub fn from_optional_directory(dir: Option<impl AsRef<Path>>) -> Option<Self> {
        dir.map(|d| Self::from_directory(d))
            .filter(|mw| !mw.pages.is_empty())
    }
}

impl Middleware for ErrorPagesMiddleware {
    fn name(&self) -> &'static str {
        "error_pages"
    }

    fn priority(&self) -> i32 {
        90 // Run late, but before compression
    }

    fn on_request(&self, req: Request, ctx: &mut Context) -> MiddlewareResult {
        // Store whether client accepts HTML
        ctx.accepts_html = req.accepts_html();
        MiddlewareResult::Next(req)
    }

    fn on_response(&self, res: Response, ctx: &Context) -> Response {
        let status = res.status().as_u16();

        // Only handle error responses with empty body
        if status < 400 || !res.body().is_empty() {
            return res;
        }

        // Only serve HTML to clients that accept it
        if !ctx.accepts_html {
            return res;
        }

        // Try to find a custom page for this status
        if let Some(page) = self.pages.get(status) {
            tracing::debug!(status = status, "serving custom error page");
            return res
                .with_body(page.clone())
                .with_header("Content-Type", "text/html; charset=utf-8");
        }

        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn create_context(accepts_html: bool) -> Context {
        Context::builder(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))
            .trace_id("trace")
            .span_id("span")
            .accepts_html(accepts_html)
            .build()
    }

    #[test]
    fn test_error_pages_empty() {
        let pages = ErrorPages::new();
        assert!(pages.is_empty());
        assert_eq!(pages.len(), 0);
        assert!(pages.get(404).is_none());
    }

    #[test]
    fn test_middleware_skips_success() {
        let mut pages_map = HashMap::new();
        pages_map.insert(404, Bytes::from("Not Found"));
        let pages = ErrorPages {
            pages: Arc::new(pages_map),
        };
        let mw = ErrorPagesMiddleware::new(pages);

        let ctx = create_context(true);
        let res = Response::empty(http::StatusCode::OK);

        let res = mw.on_response(res, &ctx);
        assert!(res.body().is_empty()); // Should not be modified
    }

    #[test]
    fn test_middleware_skips_non_html_client() {
        let mut pages_map = HashMap::new();
        pages_map.insert(404, Bytes::from("<h1>Not Found</h1>"));
        let pages = ErrorPages {
            pages: Arc::new(pages_map),
        };
        let mw = ErrorPagesMiddleware::new(pages);

        let ctx = create_context(false); // Does not accept HTML
        let res = Response::empty(http::StatusCode::NOT_FOUND);

        let res = mw.on_response(res, &ctx);
        assert!(res.body().is_empty()); // Should not be modified
    }

    #[test]
    fn test_middleware_serves_error_page() {
        let mut pages_map = HashMap::new();
        pages_map.insert(404, Bytes::from("<h1>Not Found</h1>"));
        let pages = ErrorPages {
            pages: Arc::new(pages_map),
        };
        let mw = ErrorPagesMiddleware::new(pages);

        let ctx = create_context(true);
        let res = Response::empty(http::StatusCode::NOT_FOUND);

        let res = mw.on_response(res, &ctx);
        assert_eq!(res.body().as_ref(), b"<h1>Not Found</h1>");
        assert_eq!(res.content_type(), Some("text/html; charset=utf-8"));
    }

    #[test]
    fn test_middleware_skips_nonempty_body() {
        let mut pages_map = HashMap::new();
        pages_map.insert(404, Bytes::from("<h1>Custom 404</h1>"));
        let pages = ErrorPages {
            pages: Arc::new(pages_map),
        };
        let mw = ErrorPagesMiddleware::new(pages);

        let ctx = create_context(true);
        let res = Response::builder()
            .status(http::StatusCode::NOT_FOUND)
            .body("Custom error message")
            .build();

        let res = mw.on_response(res, &ctx);
        assert_eq!(res.body().as_ref(), b"Custom error message"); // Should not be replaced
    }

    #[test]
    fn test_middleware_extracts_accept_html() {
        let pages = ErrorPages::new();
        let mw = ErrorPagesMiddleware::new(pages);

        let mut headers = http::HeaderMap::new();
        headers.insert("accept", "text/html, application/json".parse().unwrap());

        let req = Request::new(
            http::Method::GET,
            "/test".parse().unwrap(),
            headers,
            bytes::Bytes::new(),
        );

        let mut ctx = Context::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            "trace".to_string(),
            "span".to_string(),
        );

        mw.on_request(req, &mut ctx);
        assert!(ctx.accepts_html);
    }
}
