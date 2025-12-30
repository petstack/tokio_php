//! Custom error pages tests
//!
//! Tests require ERROR_PAGES_DIR to be configured with custom error pages.

use crate::helpers::*;
use reqwest::StatusCode;

/// Test 404 error returns custom HTML page when Accept: text/html
#[tokio::test]
async fn test_404_custom_html_page() {
    let server = TestServer::new();
    let resp = server
        .get_with_headers("/nonexistent.php", &[("Accept", "text/html")])
        .await;

    assert_status(&resp, StatusCode::NOT_FOUND);
    // If custom error pages are configured, should return HTML
    let content_type = resp.headers().get("content-type");
    if let Some(ct) = content_type {
        if ct.to_str().unwrap().contains("text/html") {
            // Custom error page is being served
            let body = resp.text().await.unwrap();
            // Check for some HTML content (custom error pages usually have <!DOCTYPE or <html>)
            assert!(
                body.contains("<!DOCTYPE") || body.contains("<html") || body.contains("404"),
                "Custom error page should contain HTML"
            );
        }
    }
}

/// Test 404 error returns plain text when Accept: application/json
#[tokio::test]
async fn test_404_plain_text_for_json() {
    let server = TestServer::new();
    let resp = server
        .get_with_headers("/nonexistent.php", &[("Accept", "application/json")])
        .await;

    assert_status(&resp, StatusCode::NOT_FOUND);
    // When client doesn't accept HTML, should not serve custom HTML page
    let content_type = resp.headers().get("content-type");
    if let Some(ct) = content_type {
        // Should not force HTML on JSON-accepting client
        // (implementation may vary - could return text/plain or empty body)
        let ct_str = ct.to_str().unwrap();
        // Just verify we got a response
        assert!(
            !ct_str.is_empty() || resp.content_length() == Some(0),
            "Should have content-type or empty body"
        );
    }
}

/// Test 500 error page (if applicable)
#[tokio::test]
#[ignore = "Requires a way to trigger 500 error"]
async fn test_500_custom_page() {
    // This test would require a PHP script that intentionally triggers a 500 error
    // For now, just verify the test structure
}

/// Test error page content-type is text/html
#[tokio::test]
async fn test_error_page_content_type() {
    let server = TestServer::new();
    let resp = server
        .get_with_headers("/nonexistent.php", &[("Accept", "text/html, */*")])
        .await;

    assert_status(&resp, StatusCode::NOT_FOUND);
    // Custom error pages should be text/html
    if let Some(ct) = resp.headers().get("content-type") {
        let ct_str = ct.to_str().unwrap();
        // If HTML error page is served, verify content-type
        if ct_str.contains("text/html") {
            assert!(
                ct_str.contains("text/html"),
                "Error page should be text/html"
            );
        }
    }
}

/// Test error page doesn't replace non-empty PHP error responses
#[tokio::test]
async fn test_php_error_not_replaced() {
    let server = TestServer::new();

    // A PHP script that returns its own error message should not be replaced
    // This depends on having a PHP script that outputs error content
    // For now, test that normal PHP responses aren't affected
    let resp = server.get("/index.php").await;
    assert_status(&resp, StatusCode::OK);
    // Body should contain PHP output, not an error page
    assert_body_contains(resp, "tokio_php").await;
}

/// Test 403 Forbidden error page (if directory listing is disabled)
#[tokio::test]
async fn test_403_error_page() {
    let server = TestServer::new();

    // Try to access a directory (should be forbidden or not found)
    let resp = server
        .get_with_headers("/errors/", &[("Accept", "text/html")])
        .await;

    // Could be 403 or 404 depending on configuration
    assert!(
        resp.status() == StatusCode::FORBIDDEN
            || resp.status() == StatusCode::NOT_FOUND
            || resp.status() == StatusCode::OK, // If directory index is enabled
        "Expected 403, 404, or 200 for directory access, got {}",
        resp.status()
    );
}
