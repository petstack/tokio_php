//! Static file caching middleware.
//!
//! Adds Cache-Control headers to static file responses.

use std::time::Duration;

use crate::core::{Context, Response};

use super::Middleware;

/// Static file extensions that should be cached.
const CACHEABLE_EXTENSIONS: &[&str] = &[
    // Images
    "png", "jpg", "jpeg", "gif", "ico", "webp", "svg", "avif",
    // Fonts
    "woff", "woff2", "ttf", "otf", "eot",
    // Styles/Scripts
    "css", "js", "mjs",
    // Other
    "json", "xml", "txt", "pdf", "map",
];

/// Check if a path is a static file that should be cached.
fn is_static_file(path: &str) -> bool {
    if let Some(ext) = path.rsplit('.').next() {
        CACHEABLE_EXTENSIONS.contains(&ext.to_lowercase().as_str())
    } else {
        false
    }
}

/// Static cache middleware configuration.
#[derive(Clone, Debug)]
pub struct StaticCacheConfig {
    /// Time-to-live for cached static files.
    pub ttl: Duration,
    /// Whether to add immutable directive for fingerprinted assets.
    pub immutable: bool,
}

impl Default for StaticCacheConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(86400), // 1 day
            immutable: true,
        }
    }
}

/// Static file caching middleware.
///
/// Adds `Cache-Control` headers to static file responses.
pub struct StaticCacheMiddleware {
    ttl_secs: u64,
    immutable: bool,
}

impl StaticCacheMiddleware {
    /// Create with default TTL (1 day).
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with custom TTL.
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            ttl_secs: ttl.as_secs(),
            immutable: true,
        }
    }

    /// Create from config.
    pub fn from_config(config: StaticCacheConfig) -> Self {
        Self {
            ttl_secs: config.ttl.as_secs(),
            immutable: config.immutable,
        }
    }

    /// Disable immutable directive.
    pub fn without_immutable(mut self) -> Self {
        self.immutable = false;
        self
    }
}

impl Default for StaticCacheMiddleware {
    fn default() -> Self {
        Self {
            ttl_secs: 86400,
            immutable: true,
        }
    }
}

impl Middleware for StaticCacheMiddleware {
    fn name(&self) -> &'static str {
        "static_cache"
    }

    fn priority(&self) -> i32 {
        50 // After routing, before compression
    }

    fn on_response(&self, res: Response, ctx: &Context) -> Response {
        // Skip if not a successful response
        if !res.is_success() {
            return res;
        }

        // Skip if already has Cache-Control
        if res.header("cache-control").is_some() {
            return res;
        }

        // Check if this is a static file request (from context)
        let is_static = ctx.get::<bool>("is_static_file").copied().unwrap_or(false);

        // Check path from context if stored
        let path = ctx.get::<String>("request_path");
        let is_static = is_static || path.map(|p| is_static_file(p)).unwrap_or(false);

        if !is_static {
            return res;
        }

        // Build Cache-Control header
        let cache_control = if self.immutable {
            format!("public, max-age={}, immutable", self.ttl_secs)
        } else {
            format!("public, max-age={}", self.ttl_secs)
        };

        res.with_header("Cache-Control", cache_control)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn create_context_with_path(path: &str) -> Context {
        let mut ctx = Context::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            "trace".to_string(),
            "span".to_string(),
        );
        ctx.set("request_path", path.to_string());
        ctx
    }

    #[test]
    fn test_is_static_file() {
        assert!(is_static_file("/styles.css"));
        assert!(is_static_file("/app.js"));
        assert!(is_static_file("/image.png"));
        assert!(is_static_file("/font.woff2"));
        assert!(is_static_file("/data.json"));

        assert!(!is_static_file("/index.php"));
        assert!(!is_static_file("/api/users"));
        assert!(!is_static_file("/"));
    }

    #[test]
    fn test_adds_cache_control_for_static() {
        let mw = StaticCacheMiddleware::new();
        let ctx = create_context_with_path("/styles.css");

        let res = Response::ok("body");
        let res = mw.on_response(res, &ctx);

        assert!(res.header("cache-control").is_some());
        let cc = res.header("cache-control").unwrap();
        assert!(cc.contains("public"));
        assert!(cc.contains("max-age="));
        assert!(cc.contains("immutable"));
    }

    #[test]
    fn test_skips_php_files() {
        let mw = StaticCacheMiddleware::new();
        let ctx = create_context_with_path("/index.php");

        let res = Response::ok("body");
        let res = mw.on_response(res, &ctx);

        assert!(res.header("cache-control").is_none());
    }

    #[test]
    fn test_skips_error_responses() {
        let mw = StaticCacheMiddleware::new();
        let ctx = create_context_with_path("/styles.css");

        let res = Response::not_found();
        let res = mw.on_response(res, &ctx);

        assert!(res.header("cache-control").is_none());
    }

    #[test]
    fn test_respects_existing_cache_control() {
        let mw = StaticCacheMiddleware::new();
        let ctx = create_context_with_path("/styles.css");

        let res = Response::builder()
            .status(http::StatusCode::OK)
            .header("cache-control", "no-cache")
            .body("body")
            .build();

        let res = mw.on_response(res, &ctx);

        // Should not override existing header
        assert_eq!(res.header("cache-control"), Some("no-cache"));
    }

    #[test]
    fn test_custom_ttl() {
        let mw = StaticCacheMiddleware::with_ttl(Duration::from_secs(3600));
        let ctx = create_context_with_path("/app.js");

        let res = Response::ok("body");
        let res = mw.on_response(res, &ctx);

        let cc = res.header("cache-control").unwrap();
        assert!(cc.contains("max-age=3600"));
    }

    #[test]
    fn test_without_immutable() {
        let mw = StaticCacheMiddleware::new().without_immutable();
        let ctx = create_context_with_path("/styles.css");

        let res = Response::ok("body");
        let res = mw.on_response(res, &ctx);

        let cc = res.header("cache-control").unwrap();
        assert!(!cc.contains("immutable"));
    }
}
