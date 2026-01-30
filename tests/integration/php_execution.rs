//! PHP execution tests: superglobals, cookies, headers, etc.

use crate::helpers::*;
use reqwest::StatusCode;

/// Test $_GET superglobal
#[tokio::test]
async fn test_get_superglobal() {
    let server = TestServer::new();
    let resp = server.get("/hello.php?name=TestValue&foo=bar").await;

    assert_status(&resp, StatusCode::OK);
    assert_body_contains(resp, "TestValue").await;
}

/// Test $_POST superglobal
#[tokio::test]
async fn test_post_superglobal() {
    let server = TestServer::new();
    let resp = server
        .post_form(
            "/form.php",
            &[("name", "PostTest"), ("email", "test@test.com")],
        )
        .await;

    assert_status(&resp, StatusCode::OK);
    assert_body_contains(resp, "PostTest").await;
}

/// Test $_SERVER superglobal contains expected values
#[tokio::test]
async fn test_server_superglobal() {
    let server = TestServer::new();
    let resp = server.get("/server_vars.php").await;

    assert_status(&resp, StatusCode::OK);
    let body = resp.text().await.unwrap();

    // Check for common $_SERVER variables
    assert!(
        body.contains("REQUEST_METHOD") || body.contains("request_method"),
        "Should contain REQUEST_METHOD"
    );
}

/// Test $_COOKIE superglobal
#[tokio::test]
async fn test_cookie_superglobal() {
    let server = TestServer::new();
    let resp = server
        .get_with_headers("/cookie.php", &[("Cookie", "test_cookie=cookie_value")])
        .await;

    assert_status(&resp, StatusCode::OK);
    // The page should show the cookie
    assert_body_contains(resp, "cookie_value").await;
}

/// Test Set-Cookie header from PHP
#[tokio::test]
async fn test_php_set_cookie() {
    let server = TestServer::new();
    let resp = server.get("/cookie.php?action=set").await;

    assert_status(&resp, StatusCode::OK);
    // Check if Set-Cookie header is present
    assert_has_header(&resp, "set-cookie");
}

/// Test PHP output (bench.php returns "ok")
#[tokio::test]
async fn test_php_output() {
    let server = TestServer::new();
    let resp = server.get("/bench.php").await;

    assert_status(&resp, StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert_eq!(body.trim(), "ok");
}

/// Test phpinfo() output
#[tokio::test]
async fn test_phpinfo() {
    let server = TestServer::new();
    let resp = server.get("/info.php").await;

    assert_status(&resp, StatusCode::OK);
    assert_body_contains(resp, "PHP Version").await;
}

/// Test PHP version in response
#[tokio::test]
async fn test_php_version() {
    let server = TestServer::new();
    let resp = server.get("/index.php").await;

    assert_status(&resp, StatusCode::OK);
    // index.php displays PHP version
    let body = resp.text().await.unwrap();
    // Should contain version like 8.4 or 8.5
    assert!(
        body.contains("8.4") || body.contains("8.5") || body.contains("PHP"),
        "Should contain PHP version"
    );
}

/// Test OPcache status endpoint
#[tokio::test]
async fn test_opcache_status() {
    let server = TestServer::new();
    let resp = server.get("/opcache_status.php").await;

    assert_status(&resp, StatusCode::OK);
    // Should return JSON with opcache info
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("opcache_enabled") || body.contains("OPcache"),
        "Should contain OPcache info"
    );
}

/// Test extension test endpoint
#[tokio::test]
async fn test_extension() {
    let server = TestServer::new();
    let resp = server.get("/ext_test.php").await;

    assert_status(&resp, StatusCode::OK);
    // Should return some response from ext_test.php
    let body = resp.text().await.unwrap();
    assert!(!body.is_empty(), "ext_test.php should return content");
}

/// Test HTTP method detection in PHP
#[tokio::test]
async fn test_request_method_get() {
    let server = TestServer::new();
    let resp = server.get("/form.php").await;

    assert_status(&resp, StatusCode::OK);
    // form.php shows different content for GET vs POST
    let body = resp.text().await.unwrap();
    // GET request should not show "Received:" message
    assert!(
        !body.contains("Received: Name"),
        "GET request should not show POST result"
    );
}

/// Test HTTP method detection in PHP (POST)
#[tokio::test]
async fn test_request_method_post() {
    let server = TestServer::new();
    let resp = server
        .post_form("/form.php", &[("name", "Test"), ("email", "test@test.com")])
        .await;

    assert_status(&resp, StatusCode::OK);
    // POST request should show "Received:" message
    assert_body_contains(resp, "Received:").await;
}

/// Test empty POST body
#[tokio::test]
async fn test_empty_post() {
    let server = TestServer::new();
    let resp = server.post_form("/form.php", &[]).await;

    assert_status(&resp, StatusCode::OK);
}

/// Test large query string
#[tokio::test]
async fn test_large_query_string() {
    let server = TestServer::new();
    let long_value = "x".repeat(1000);
    let resp = server.get(&format!("/hello.php?name={}", long_value)).await;

    assert_status(&resp, StatusCode::OK);
}

/// Test Unicode in parameters
#[tokio::test]
async fn test_unicode_params() {
    let server = TestServer::new();
    let resp = server.get("/hello.php?name=%E4%B8%AD%E6%96%87").await;

    assert_status(&resp, StatusCode::OK);
    // Should contain the Chinese characters or at least not crash
}

/// Test concurrent PHP requests
#[tokio::test]
async fn test_concurrent_requests() {
    let server = TestServer::new();

    // Send 10 concurrent requests
    let mut handles = Vec::new();
    for i in 0..10 {
        let client = server.client.clone();
        let url = format!("{}/hello.php?name=User{}", server.base_url, i);
        handles.push(tokio::spawn(async move { client.get(&url).send().await }));
    }

    // All should succeed
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        let resp = result.expect("Request failed");
        assert_status(&resp, StatusCode::OK);
    }
}

/// Test $_SERVER['REQUEST_TIME'] and $_SERVER['REQUEST_TIME_FLOAT']
/// Verifies the SAPI get_request_time callback works correctly
#[tokio::test]
async fn test_request_time() {
    let server = TestServer::new();
    let resp = server.get("/test_request_time.php").await;

    assert_status(&resp, StatusCode::OK);
    let body = resp.text().await.unwrap();

    // Parse JSON response
    let data: serde_json::Value = serde_json::from_str(&body).expect("Invalid JSON response");

    // REQUEST_TIME should be a positive integer (Unix timestamp)
    let request_time = data["request_time"].as_i64().unwrap_or(0);
    assert!(
        request_time > 0,
        "REQUEST_TIME should be positive, got: {}",
        request_time
    );

    // REQUEST_TIME_FLOAT should be a positive float with microsecond precision
    let request_time_float = data["request_time_float"].as_f64().unwrap_or(0.0);
    assert!(
        request_time_float > 0.0,
        "REQUEST_TIME_FLOAT should be positive, got: {}",
        request_time_float
    );

    // REQUEST_TIME_FLOAT should have fractional component (microseconds)
    let has_fractional = (request_time_float - request_time_float.floor()).abs() > 0.0;
    assert!(
        has_fractional,
        "REQUEST_TIME_FLOAT should have microseconds, got: {}",
        request_time_float
    );

    // delay_ms shows time between request reception and PHP execution
    // Should be positive and reasonable (< 5000ms)
    let delay_ms = data["delay_ms"].as_f64().unwrap_or(-1.0);
    assert!(
        (0.0..5000.0).contains(&delay_ms),
        "delay_ms should be 0-5000ms, got: {}",
        delay_ms
    );

    // is_valid should be true if all checks passed in PHP
    let is_valid = data["is_valid"].as_bool().unwrap_or(false);
    assert!(is_valid, "PHP validation failed: {}", body);
}
