//! Basic HTTP tests: GET, POST, HEAD, 404, etc.

use crate::helpers::*;
use reqwest::StatusCode;

/// Test GET request to index.php
#[tokio::test]
async fn test_get_index() {
    let server = TestServer::new();
    let resp = server.get("/index.php").await;

    assert_status(&resp, StatusCode::OK);
    assert_header_starts_with(&resp, "content-type", "text/html");
    assert_body_contains(resp, "tokio_php").await;
}

/// Test GET request with query parameters
#[tokio::test]
async fn test_get_with_query_params() {
    let server = TestServer::new();
    let resp = server.get("/hello.php?name=TestUser").await;

    assert_status(&resp, StatusCode::OK);
    assert_body_contains(resp, "Hello, TestUser!").await;
}

/// Test GET request with multiple query params
#[tokio::test]
async fn test_get_with_multiple_params() {
    let server = TestServer::new();
    let resp = server.get("/hello.php?name=Alice&greeting=Hi").await;

    assert_status(&resp, StatusCode::OK);
    assert_body_contains(resp, "Alice").await;
}

/// Test POST request with form data
#[tokio::test]
async fn test_post_form_data() {
    let server = TestServer::new();
    let resp = server
        .post_form("/form.php", &[("name", "John"), ("email", "john@example.com")])
        .await;

    assert_status(&resp, StatusCode::OK);
    assert_body_contains(resp, "Name = 'John'").await;
}

/// Test 404 for non-existent file
#[tokio::test]
async fn test_404_not_found() {
    let server = TestServer::new();
    let resp = server.get("/nonexistent.php").await;

    assert_status(&resp, StatusCode::NOT_FOUND);
}

/// Test 404 for non-existent path
#[tokio::test]
async fn test_404_path_not_found() {
    let server = TestServer::new();
    let resp = server.get("/path/to/nowhere").await;

    assert_status(&resp, StatusCode::NOT_FOUND);
}

/// Test HEAD request
#[tokio::test]
async fn test_head_request() {
    let server = TestServer::new();
    let resp = server
        .client
        .head(format!("{}/index.php", server.base_url))
        .send()
        .await
        .expect("HEAD request failed");

    assert_status(&resp, StatusCode::OK);
    assert_has_header(&resp, "content-type");
}

/// Test X-Request-ID header is present
#[tokio::test]
async fn test_request_id_header() {
    let server = TestServer::new();
    let resp = server.get("/bench.php").await;

    assert_status(&resp, StatusCode::OK);
    assert_has_header(&resp, "x-request-id");
}

/// Test X-Request-ID propagation
#[tokio::test]
async fn test_request_id_propagation() {
    let server = TestServer::new();
    let custom_id = "my-custom-request-id-123";
    let resp = server
        .get_with_headers("/bench.php", &[("X-Request-ID", custom_id)])
        .await;

    assert_status(&resp, StatusCode::OK);
    assert_header(&resp, "x-request-id", custom_id);
}

/// Test simple PHP execution (bench.php returns "ok")
#[tokio::test]
async fn test_simple_php_execution() {
    let server = TestServer::new();
    let resp = server.get("/bench.php").await;

    assert_status(&resp, StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert_eq!(body.trim(), "ok");
}

/// Test URL-encoded query parameters
#[tokio::test]
async fn test_url_encoded_params() {
    let server = TestServer::new();
    let resp = server.get("/hello.php?name=Hello%20World").await;

    assert_status(&resp, StatusCode::OK);
    assert_body_contains(resp, "Hello, Hello World!").await;
}

/// Test special characters in query params
#[tokio::test]
async fn test_special_chars_in_params() {
    let server = TestServer::new();
    let resp = server.get("/hello.php?name=%3Cscript%3E").await;

    assert_status(&resp, StatusCode::OK);
    // Should be HTML-escaped
    assert_body_contains(resp, "&lt;script&gt;").await;
}
