//! Request routing and path resolution.

use std::path::Path;
use std::sync::Arc;

/// Resolve a URI path to a file path.
///
/// Handles URL decoding, path traversal prevention, and directory index files.
#[inline]
pub fn resolve_file_path(
    uri_path: &str,
    document_root: &str,
    index_file_path: Option<&Arc<str>>,
) -> String {
    // In single entry point mode with try_files behavior
    if let Some(idx_path) = index_file_path {
        // First, try the actual URI path (try_files behavior)
        let static_path = resolve_uri_to_path(uri_path, document_root);
        let path = Path::new(&static_path);

        // Check if file exists, is a file (not directory), and is not .php
        if path.is_file() {
            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if extension != "php" {
                return static_path;
            }
        }

        // Fall back to index file
        return idx_path.to_string();
    }

    resolve_uri_to_path(uri_path, document_root)
}

/// Resolve URI to file path without index_file consideration.
#[inline]
fn resolve_uri_to_path(uri_path: &str, document_root: &str) -> String {
    let decoded_path = percent_encoding::percent_decode_str(uri_path).decode_utf8_lossy();

    let clean_path = decoded_path.trim_start_matches('/').replace("..", "");

    if clean_path.is_empty() || clean_path.ends_with('/') {
        format!("{}/{}/index.php", document_root, clean_path)
    } else {
        format!("{}/{}", document_root, clean_path)
    }
}

/// Check if a URI path looks like it should be handled as PHP.
#[inline]
pub fn is_php_uri(uri_path: &str) -> bool {
    uri_path.ends_with(".php") || uri_path.ends_with('/') || uri_path == "/"
}

/// Check if the request is trying to directly access the index file.
///
/// Returns true if access should be blocked (returns 404).
#[inline]
pub fn is_direct_index_access(uri_path: &str, index_file_name: Option<&Arc<str>>) -> bool {
    if let Some(idx_name) = index_file_name {
        let direct_path = format!("/{}", idx_name.as_ref());
        uri_path == direct_path || uri_path.starts_with(&format!("{}/", direct_path))
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // resolve_uri_to_path tests
    // ========================================

    #[test]
    fn test_resolve_uri_basic_php_file() {
        let result = resolve_uri_to_path("/index.php", "/var/www/html");
        assert_eq!(result, "/var/www/html/index.php");
    }

    #[test]
    fn test_resolve_uri_nested_path() {
        let result = resolve_uri_to_path("/api/users/list.php", "/var/www/html");
        assert_eq!(result, "/var/www/html/api/users/list.php");
    }

    #[test]
    fn test_resolve_uri_root_path() {
        let result = resolve_uri_to_path("/", "/var/www/html");
        assert_eq!(result, "/var/www/html//index.php");
    }

    #[test]
    fn test_resolve_uri_trailing_slash() {
        let result = resolve_uri_to_path("/admin/", "/var/www/html");
        assert_eq!(result, "/var/www/html/admin//index.php");
    }

    #[test]
    fn test_resolve_uri_url_decoding() {
        let result = resolve_uri_to_path("/path%20with%20spaces.php", "/var/www/html");
        assert_eq!(result, "/var/www/html/path with spaces.php");
    }

    #[test]
    fn test_resolve_uri_url_decoding_unicode() {
        let result = resolve_uri_to_path("/%D1%82%D0%B5%D1%81%D1%82.php", "/var/www/html");
        assert_eq!(result, "/var/www/html/тест.php");
    }

    #[test]
    fn test_resolve_uri_path_traversal_prevention() {
        // .. should be removed for security
        let result = resolve_uri_to_path("/../etc/passwd", "/var/www/html");
        assert_eq!(result, "/var/www/html//etc/passwd");
    }

    #[test]
    fn test_resolve_uri_path_traversal_middle() {
        let result = resolve_uri_to_path("/admin/../config.php", "/var/www/html");
        assert_eq!(result, "/var/www/html/admin//config.php");
    }

    #[test]
    fn test_resolve_uri_static_file() {
        let result = resolve_uri_to_path("/css/style.css", "/var/www/html");
        assert_eq!(result, "/var/www/html/css/style.css");
    }

    #[test]
    fn test_resolve_uri_empty_path() {
        let result = resolve_uri_to_path("", "/var/www/html");
        assert_eq!(result, "/var/www/html//index.php");
    }

    // ========================================
    // resolve_file_path tests (without index_file)
    // ========================================

    #[test]
    fn test_resolve_file_path_no_index_file() {
        let result = resolve_file_path("/test.php", "/var/www/html", None);
        assert_eq!(result, "/var/www/html/test.php");
    }

    #[test]
    fn test_resolve_file_path_no_index_file_nested() {
        let result = resolve_file_path("/api/v1/users.php", "/var/www/html", None);
        assert_eq!(result, "/var/www/html/api/v1/users.php");
    }

    // ========================================
    // is_php_uri tests
    // ========================================

    #[test]
    fn test_is_php_uri_php_extension() {
        assert!(is_php_uri("/index.php"));
        assert!(is_php_uri("/api/users.php"));
        assert!(is_php_uri("/deep/nested/path/script.php"));
    }

    #[test]
    fn test_is_php_uri_trailing_slash() {
        assert!(is_php_uri("/"));
        assert!(is_php_uri("/admin/"));
        assert!(is_php_uri("/api/v1/"));
    }

    #[test]
    fn test_is_php_uri_root() {
        assert!(is_php_uri("/"));
    }

    #[test]
    fn test_is_php_uri_static_files() {
        assert!(!is_php_uri("/style.css"));
        assert!(!is_php_uri("/script.js"));
        assert!(!is_php_uri("/image.png"));
        assert!(!is_php_uri("/favicon.ico"));
        assert!(!is_php_uri("/robots.txt"));
    }

    #[test]
    fn test_is_php_uri_no_extension() {
        // Paths without extension or trailing slash are NOT php
        assert!(!is_php_uri("/api/users"));
        assert!(!is_php_uri("/health"));
    }

    #[test]
    fn test_is_php_uri_php_in_path() {
        // .php only matches at the end
        assert!(!is_php_uri("/php/config"));
        assert!(!is_php_uri("/test.php.bak"));
    }

    // ========================================
    // is_direct_index_access tests
    // ========================================

    #[test]
    fn test_is_direct_index_access_exact_match() {
        let index_name: Arc<str> = Arc::from("index.php");
        assert!(is_direct_index_access("/index.php", Some(&index_name)));
    }

    #[test]
    fn test_is_direct_index_access_with_subpath() {
        let index_name: Arc<str> = Arc::from("index.php");
        // /index.php/foo should be blocked (path info after index)
        assert!(is_direct_index_access("/index.php/foo", Some(&index_name)));
        assert!(is_direct_index_access(
            "/index.php/api/users",
            Some(&index_name)
        ));
    }

    #[test]
    fn test_is_direct_index_access_no_index_configured() {
        assert!(!is_direct_index_access("/index.php", None));
        assert!(!is_direct_index_access("/anything", None));
    }

    #[test]
    fn test_is_direct_index_access_other_paths() {
        let index_name: Arc<str> = Arc::from("index.php");
        assert!(!is_direct_index_access("/", Some(&index_name)));
        assert!(!is_direct_index_access("/api/users", Some(&index_name)));
        assert!(!is_direct_index_access("/other.php", Some(&index_name)));
        assert!(!is_direct_index_access(
            "/admin/index.php",
            Some(&index_name)
        ));
    }

    #[test]
    fn test_is_direct_index_access_custom_index() {
        let index_name: Arc<str> = Arc::from("app.php");
        assert!(is_direct_index_access("/app.php", Some(&index_name)));
        assert!(is_direct_index_access("/app.php/route", Some(&index_name)));
        assert!(!is_direct_index_access("/index.php", Some(&index_name)));
    }

    #[test]
    fn test_is_direct_index_access_similar_names() {
        let index_name: Arc<str> = Arc::from("index.php");
        // Should not match partial names
        assert!(!is_direct_index_access("/index.phps", Some(&index_name)));
        assert!(!is_direct_index_access("/index.php2", Some(&index_name)));
        assert!(!is_direct_index_access("/myindex.php", Some(&index_name)));
    }

    // ========================================
    // Edge cases and security tests
    // ========================================

    #[test]
    fn test_percent_encoded_path_traversal() {
        // %2e%2e is URL-encoded ".."
        let result = resolve_uri_to_path("/%2e%2e/etc/passwd", "/var/www/html");
        // After decoding, .. should be removed
        assert_eq!(result, "/var/www/html//etc/passwd");
    }

    #[test]
    fn test_double_encoded_path() {
        // %252e%252e is double-encoded ".."
        // First decode: %2e%2e, but we only decode once
        let result = resolve_uri_to_path("/%252e%252e/etc/passwd", "/var/www/html");
        // %25 decodes to %, so we get %2e%2e which is NOT decoded again
        assert_eq!(result, "/var/www/html/%2e%2e/etc/passwd");
    }

    #[test]
    fn test_null_byte_injection() {
        // Null bytes in path
        let result = resolve_uri_to_path("/test%00.php", "/var/www/html");
        assert_eq!(result, "/var/www/html/test\0.php");
    }

    #[test]
    fn test_very_long_path() {
        let long_segment = "a".repeat(255);
        let uri = format!("/{}/{}.php", long_segment, long_segment);
        let result = resolve_uri_to_path(&uri, "/var/www/html");
        assert!(result.starts_with("/var/www/html/"));
        assert!(result.ends_with(".php"));
    }

    #[test]
    fn test_special_characters_in_path() {
        let result = resolve_uri_to_path("/path[with]special{chars}.php", "/var/www/html");
        assert_eq!(result, "/var/www/html/path[with]special{chars}.php");
    }

    #[test]
    fn test_query_string_not_in_path() {
        // Note: query string should be stripped before calling resolve_uri_to_path
        // This test documents current behavior (query string IS included if passed)
        let result = resolve_uri_to_path("/test.php?foo=bar", "/var/www/html");
        assert_eq!(result, "/var/www/html/test.php?foo=bar");
    }
}
