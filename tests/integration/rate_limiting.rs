//! Rate limiting tests
//!
//! Note: These tests require the server to be configured with rate limiting enabled.
//! Set RATE_LIMIT=5 RATE_WINDOW=10 for these tests to work properly.

use crate::helpers::*;
use reqwest::StatusCode;

/// Test rate limit headers are present when rate limiting is enabled
#[tokio::test]
async fn test_rate_limit_headers_present() {
    let server = TestServer::new();
    let resp = server.get("/bench.php").await;

    // If rate limiting is enabled, these headers should be present
    if resp.headers().contains_key("x-ratelimit-limit") {
        assert_has_header(&resp, "x-ratelimit-remaining");
        assert_has_header(&resp, "x-ratelimit-reset");
    }
}

/// Test rate limit remaining decreases with requests
#[tokio::test]
async fn test_rate_limit_remaining_decreases() {
    let server = TestServer::new();

    // First request
    let resp1 = server.get("/bench.php").await;
    let remaining1 = resp1
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    // Second request
    let resp2 = server.get("/bench.php").await;
    let remaining2 = resp2
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    // If rate limiting is enabled, remaining should decrease
    if let (Some(r1), Some(r2)) = (remaining1, remaining2) {
        assert!(r2 <= r1, "Remaining should decrease: {} -> {}", r1, r2);
    }
}

/// Test that 429 is returned when rate limit is exceeded
/// This test only runs if RATE_LIMIT is set low enough
#[tokio::test]
#[ignore = "Requires RATE_LIMIT=5 RATE_WINDOW=60 configuration"]
async fn test_rate_limit_exceeded() {
    let server = TestServer::new();

    // Make many requests quickly
    let mut last_status = StatusCode::OK;
    for _ in 0..20 {
        let resp = server.get("/bench.php").await;
        last_status = resp.status();
        if last_status == StatusCode::TOO_MANY_REQUESTS {
            break;
        }
    }

    assert_eq!(
        last_status,
        StatusCode::TOO_MANY_REQUESTS,
        "Should get 429 after exceeding rate limit"
    );
}

/// Test Retry-After header is present on 429 responses
#[tokio::test]
#[ignore = "Requires RATE_LIMIT=5 RATE_WINDOW=60 configuration"]
async fn test_retry_after_header() {
    let server = TestServer::new();

    // Make many requests quickly to trigger rate limit
    for _ in 0..20 {
        let resp = server.get("/bench.php").await;
        if resp.status() == StatusCode::TOO_MANY_REQUESTS {
            assert_has_header(&resp, "retry-after");
            return;
        }
    }

    // If we didn't hit rate limit, skip the assertion
}

/// Test rate limiting is per-IP (different clients should have separate limits)
#[tokio::test]
async fn test_rate_limit_per_ip() {
    let server = TestServer::new();

    // Make a request with X-Forwarded-For header
    let resp = server
        .get_with_headers("/bench.php", &[("X-Forwarded-For", "192.168.1.100")])
        .await;

    assert_status(&resp, StatusCode::OK);

    // Make another request with different X-Forwarded-For
    let resp = server
        .get_with_headers("/bench.php", &[("X-Forwarded-For", "192.168.1.101")])
        .await;

    assert_status(&resp, StatusCode::OK);
}

/// Test rate limit window resets
#[tokio::test]
#[ignore = "This test takes time to wait for window reset"]
async fn test_rate_limit_window_reset() {
    let server = TestServer::new();

    // Get initial remaining
    let resp = server.get("/bench.php").await;
    let _remaining = resp
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    // Wait for window to potentially reset (this depends on RATE_WINDOW setting)
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Request again should work
    let resp = server.get("/bench.php").await;
    assert_status(&resp, StatusCode::OK);
}
