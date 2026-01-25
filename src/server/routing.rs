//! Request routing and path resolution.
//!
//! Implements nginx-style try_files behavior for PHP applications.

use std::sync::Arc;

use super::file_cache::{FileCache, FileType};

/// Route configuration.
#[derive(Debug, Clone)]
pub struct RouteConfig {
    /// Document root directory (e.g., "/var/www/html")
    pub document_root: Arc<str>,
    /// Index file name (e.g., "index.php" or "index.html")
    pub index_file: Option<Arc<str>>,
    /// Full path to index file (e.g., "/var/www/html/index.php")
    pub index_file_path: Option<Arc<str>>,
    /// Whether index file is PHP
    pub index_file_is_php: bool,
}

impl RouteConfig {
    /// Create a new route configuration.
    pub fn new(document_root: &str, index_file: Option<&str>) -> Self {
        let document_root: Arc<str> = Arc::from(document_root);
        let (index_file, index_file_path, index_file_is_php) = match index_file {
            Some(name) => {
                let full_path = format!("{}/{}", document_root, name);
                let is_php = name.ends_with(".php");
                (
                    Some(Arc::from(name)),
                    Some(Arc::from(full_path.as_str())),
                    is_php,
                )
            }
            None => (None, None, false),
        };

        Self {
            document_root,
            index_file,
            index_file_path,
            index_file_is_php,
        }
    }
}

/// Result of route resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteResult {
    /// Execute PHP script at given path
    Execute(String),
    /// Serve static file at given path
    Serve(String),
    /// Return 404 Not Found
    NotFound,
}

/// Resolve a request URI to a route result.
///
/// Implements the routing logic:
/// 1. Direct access to INDEX_FILE -> 404
/// 2. INDEX_FILE=*.php and uri=*.php -> 404
/// 3. Trailing slash -> directory mode
/// 4. File exists -> serve/execute
/// 5. INDEX_FILE set -> fallback to INDEX_FILE
/// 6. -> 404
#[inline]
pub fn resolve_request(uri_path: &str, config: &RouteConfig, cache: &FileCache) -> RouteResult {
    // 1. Decode URI and sanitize
    let decoded = percent_encoding::percent_decode_str(uri_path).decode_utf8_lossy();
    let safe_path = sanitize_path(&decoded);

    // 2. Check direct access to INDEX_FILE -> 404
    if is_direct_index_access(&safe_path, config) {
        return RouteResult::NotFound;
    }

    // 3. INDEX_FILE=*.php and uri=*.php -> 404
    if config.index_file_is_php && safe_path.ends_with(".php") {
        return RouteResult::NotFound;
    }

    // 4. Root path "/"
    if safe_path == "/" || safe_path.is_empty() {
        return resolve_root(config, cache);
    }

    // 5. Trailing slash -> directory mode
    if safe_path.ends_with('/') {
        return resolve_directory(&safe_path, config, cache);
    }

    // 6. Normal file path
    resolve_file(&safe_path, config, cache)
}

/// Resolve root path "/".
fn resolve_root(config: &RouteConfig, cache: &FileCache) -> RouteResult {
    // INDEX_FILE set -> use it
    if let Some(ref path) = config.index_file_path {
        return if config.index_file_is_php {
            RouteResult::Execute(path.to_string())
        } else {
            RouteResult::Serve(path.to_string())
        };
    }

    // Traditional mode: index.php -> index.html -> 404
    let index_php = format!("{}/index.php", config.document_root);
    if cache.is_file(&index_php) {
        return RouteResult::Execute(index_php);
    }

    let index_html = format!("{}/index.html", config.document_root);
    if cache.is_file(&index_html) {
        return RouteResult::Serve(index_html);
    }

    RouteResult::NotFound
}

/// Resolve directory path (ends with "/").
fn resolve_directory(path: &str, config: &RouteConfig, cache: &FileCache) -> RouteResult {
    let dir_path = format!("{}{}", config.document_root, path.trim_end_matches('/'));

    // INDEX_FILE set -> look for it in directory
    if let Some(ref index_file) = config.index_file {
        let file_path = format!("{}/{}", dir_path, index_file);
        if cache.is_file(&file_path) {
            return if config.index_file_is_php {
                RouteResult::Execute(file_path)
            } else {
                RouteResult::Serve(file_path)
            };
        }
        return RouteResult::NotFound;
    }

    // Traditional mode: index.php -> index.html -> 404
    let index_php = format!("{}/index.php", dir_path);
    if cache.is_file(&index_php) {
        return RouteResult::Execute(index_php);
    }

    let index_html = format!("{}/index.html", dir_path);
    if cache.is_file(&index_html) {
        return RouteResult::Serve(index_html);
    }

    RouteResult::NotFound
}

/// Resolve regular file path (no trailing slash).
fn resolve_file(path: &str, config: &RouteConfig, cache: &FileCache) -> RouteResult {
    let full_path = format!("{}{}", config.document_root, path);

    // Check file type
    match cache.check(&full_path).0 {
        Some(FileType::File) => {
            // File exists
            if full_path.ends_with(".php") {
                RouteResult::Execute(full_path)
            } else {
                RouteResult::Serve(full_path)
            }
        }
        Some(FileType::Dir) => {
            // Directory without trailing slash -> 404 (no redirect)
            RouteResult::NotFound
        }
        None => {
            // File doesn't exist -> fallback to INDEX_FILE
            if let Some(ref idx_path) = config.index_file_path {
                if config.index_file_is_php {
                    RouteResult::Execute(idx_path.to_string())
                } else {
                    RouteResult::Serve(idx_path.to_string())
                }
            } else {
                RouteResult::NotFound
            }
        }
    }
}

/// Sanitize path: remove ".." sequences for security.
#[inline]
fn sanitize_path(path: &str) -> String {
    path.replace("..", "")
}

/// Check if request is direct access to INDEX_FILE.
#[inline]
fn is_direct_index_access(uri_path: &str, config: &RouteConfig) -> bool {
    if let Some(ref idx_name) = config.index_file {
        let direct_path = format!("/{}", idx_name.as_ref());
        uri_path == direct_path || uri_path.starts_with(&format!("{}/", direct_path))
    } else {
        false
    }
}

/// Check if a URI path looks like it should be handled as PHP.
/// Used for determining whether to send request to PHP executor.
#[inline]
pub fn is_php_uri(uri_path: &str) -> bool {
    uri_path.ends_with(".php") || uri_path.ends_with('/') || uri_path == "/"
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // RouteConfig tests
    // ========================================

    #[test]
    fn test_route_config_with_php_index() {
        let config = RouteConfig::new("/var/www/html", Some("index.php"));
        assert_eq!(config.document_root.as_ref(), "/var/www/html");
        assert_eq!(
            config.index_file.as_ref().map(|s| s.as_ref()),
            Some("index.php")
        );
        assert_eq!(
            config.index_file_path.as_ref().map(|s| s.as_ref()),
            Some("/var/www/html/index.php")
        );
        assert!(config.index_file_is_php);
    }

    #[test]
    fn test_route_config_with_html_index() {
        let config = RouteConfig::new("/var/www/html", Some("index.html"));
        assert!(!config.index_file_is_php);
    }

    #[test]
    fn test_route_config_no_index() {
        let config = RouteConfig::new("/var/www/html", None);
        assert!(config.index_file.is_none());
        assert!(config.index_file_path.is_none());
        assert!(!config.index_file_is_php);
    }

    // ========================================
    // is_direct_index_access tests
    // ========================================

    #[test]
    fn test_direct_index_access_exact() {
        let config = RouteConfig::new("/var/www/html", Some("index.php"));
        assert!(is_direct_index_access("/index.php", &config));
    }

    #[test]
    fn test_direct_index_access_with_subpath() {
        let config = RouteConfig::new("/var/www/html", Some("index.php"));
        assert!(is_direct_index_access("/index.php/foo", &config));
        assert!(is_direct_index_access("/index.php/api/users", &config));
    }

    #[test]
    fn test_direct_index_access_other_paths() {
        let config = RouteConfig::new("/var/www/html", Some("index.php"));
        assert!(!is_direct_index_access("/", &config));
        assert!(!is_direct_index_access("/api/users", &config));
        assert!(!is_direct_index_access("/other.php", &config));
        assert!(!is_direct_index_access("/admin/index.php", &config));
    }

    #[test]
    fn test_direct_index_access_no_index() {
        let config = RouteConfig::new("/var/www/html", None);
        assert!(!is_direct_index_access("/index.php", &config));
    }

    // ========================================
    // is_php_uri tests
    // ========================================

    #[test]
    fn test_is_php_uri() {
        assert!(is_php_uri("/index.php"));
        assert!(is_php_uri("/api/users.php"));
        assert!(is_php_uri("/"));
        assert!(is_php_uri("/admin/"));

        assert!(!is_php_uri("/style.css"));
        assert!(!is_php_uri("/script.js"));
        assert!(!is_php_uri("/api/users"));
    }

    // ========================================
    // sanitize_path tests
    // ========================================

    #[test]
    fn test_sanitize_path() {
        assert_eq!(sanitize_path("/etc/passwd"), "/etc/passwd");
        assert_eq!(sanitize_path("/../etc/passwd"), "//etc/passwd");
        assert_eq!(sanitize_path("/admin/../config.php"), "/admin//config.php");
    }

    // ========================================
    // Integration tests with mock filesystem
    // ========================================

    // Note: Full integration tests require actual filesystem.
    // These tests verify logic with mocked cache responses.
}
