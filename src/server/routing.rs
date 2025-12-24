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
    // In single entry point mode, always use the pre-validated index file
    if let Some(idx_path) = index_file_path {
        return idx_path.to_string();
    }

    let decoded_path = percent_encoding::percent_decode_str(uri_path).decode_utf8_lossy();

    let clean_path = decoded_path.trim_start_matches('/').replace("..", "");

    if clean_path.is_empty() || clean_path.ends_with('/') {
        format!("{}/{}/index.php", document_root, clean_path)
    } else {
        format!("{}/{}", document_root, clean_path)
    }
}

/// Check if a path is a PHP file.
#[inline]
pub fn is_php_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e == "php")
        .unwrap_or(false)
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
