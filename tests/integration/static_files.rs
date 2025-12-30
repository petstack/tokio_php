//! Static file serving tests

use crate::helpers::*;
use reqwest::StatusCode;

/// Test serving CSS file
#[tokio::test]
async fn test_serve_css_file() {
    let server = TestServer::new();
    let resp = server.get("/styles.css").await;

    assert_status(&resp, StatusCode::OK);
    assert_header_starts_with(&resp, "content-type", "text/css");
}

/// Test serving CSS file with proper caching headers
#[tokio::test]
async fn test_static_file_cache_headers() {
    let server = TestServer::new();
    let resp = server.get("/styles.css").await;

    assert_status(&resp, StatusCode::OK);
    // Should have Cache-Control header (from STATIC_CACHE_TTL)
    assert_has_header(&resp, "cache-control");
}

/// Test ETag header on static files
#[tokio::test]
async fn test_static_file_etag() {
    let server = TestServer::new();
    let resp = server.get("/styles.css").await;

    assert_status(&resp, StatusCode::OK);
    assert_has_header(&resp, "etag");
}

/// Test Content-Length header on static files
#[tokio::test]
async fn test_static_file_content_length() {
    let server = TestServer::new();
    let resp = server.get("/styles.css").await;

    assert_status(&resp, StatusCode::OK);
    assert_has_header(&resp, "content-length");
}

/// Test 404 for non-existent static file
#[tokio::test]
async fn test_static_file_not_found() {
    let server = TestServer::new();
    let resp = server.get("/nonexistent.css").await;

    assert_status(&resp, StatusCode::NOT_FOUND);
}

/// Test directory traversal protection
#[tokio::test]
async fn test_directory_traversal_protection() {
    let server = TestServer::new();

    // Attempt path traversal
    let resp = server.get("/../../../etc/passwd").await;
    assert!(
        resp.status() == StatusCode::NOT_FOUND
            || resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::FORBIDDEN,
        "Expected 400, 403 or 404 for path traversal, got {}",
        resp.status()
    );

    // URL-encoded traversal
    let resp = server.get("/%2e%2e/%2e%2e/etc/passwd").await;
    assert!(
        resp.status() == StatusCode::NOT_FOUND
            || resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::FORBIDDEN,
        "Expected 400, 403 or 404 for encoded path traversal, got {}",
        resp.status()
    );
}

/// Test MIME type detection
#[tokio::test]
async fn test_mime_type_detection() {
    let server = TestServer::new();

    // CSS file
    let resp = server.get("/styles.css").await;
    assert_status(&resp, StatusCode::OK);
    assert_header_starts_with(&resp, "content-type", "text/css");

    // Test another CSS file
    let resp = server.get("/test.css").await;
    assert_status(&resp, StatusCode::OK);
    assert_header_starts_with(&resp, "content-type", "text/css");
}

/// Test root path resolves to index or returns appropriate response
#[tokio::test]
async fn test_root_path() {
    let server = TestServer::new();
    let resp = server.get("/").await;

    // Should either return 200 (if directory index) or 404/403
    assert!(
        resp.status() == StatusCode::OK
            || resp.status() == StatusCode::NOT_FOUND
            || resp.status() == StatusCode::FORBIDDEN,
        "Expected 200, 403, or 404 for root path, got {}",
        resp.status()
    );
}
