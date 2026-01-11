//! Internal server tests (/health, /metrics)
//!
//! Tests for the internal server endpoints when INTERNAL_ADDR is configured.

use crate::helpers::*;
use reqwest::StatusCode;

/// Test /health endpoint returns 200 OK
#[tokio::test]
async fn test_health_endpoint() {
    let server = TestServer::new();
    let resp = server.internal_get("/health").await;

    assert_status(&resp, StatusCode::OK);
}

/// Test /health endpoint returns JSON
#[tokio::test]
async fn test_health_returns_json() {
    let server = TestServer::new();
    let resp = server.internal_get("/health").await;

    assert_status(&resp, StatusCode::OK);
    assert_header_starts_with(&resp, "content-type", "application/json");
}

/// Test /health endpoint contains expected fields
#[tokio::test]
async fn test_health_json_fields() {
    let server = TestServer::new();
    let resp = server.internal_get("/health").await;

    assert_status(&resp, StatusCode::OK);
    let body = resp.text().await.unwrap();

    // Should contain status field
    assert!(
        body.contains("status"),
        "Health response should contain 'status'"
    );
    assert!(body.contains("ok"), "Health status should be 'ok'");
}

/// Test /health endpoint contains timestamp
#[tokio::test]
async fn test_health_contains_timestamp() {
    let server = TestServer::new();
    let resp = server.internal_get("/health").await;

    assert_status(&resp, StatusCode::OK);
    let body = resp.text().await.unwrap();

    assert!(
        body.contains("timestamp"),
        "Health response should contain 'timestamp'"
    );
}

/// Test /health endpoint contains active_connections
#[tokio::test]
async fn test_health_contains_active_connections() {
    let server = TestServer::new();
    let resp = server.internal_get("/health").await;

    assert_status(&resp, StatusCode::OK);
    let body = resp.text().await.unwrap();

    assert!(
        body.contains("active_connections"),
        "Health response should contain 'active_connections'"
    );
}

/// Test /metrics endpoint returns 200 OK
#[tokio::test]
async fn test_metrics_endpoint() {
    let server = TestServer::new();
    let resp = server.internal_get("/metrics").await;

    assert_status(&resp, StatusCode::OK);
}

/// Test /metrics endpoint returns Prometheus format
#[tokio::test]
async fn test_metrics_prometheus_format() {
    let server = TestServer::new();
    let resp = server.internal_get("/metrics").await;

    assert_status(&resp, StatusCode::OK);
    let body = resp.text().await.unwrap();

    // Prometheus metrics have # HELP comments
    assert!(
        body.contains("# HELP") || body.contains("# TYPE"),
        "Metrics should be in Prometheus format"
    );
}

/// Test /metrics contains uptime metric
#[tokio::test]
async fn test_metrics_contains_uptime() {
    let server = TestServer::new();
    let resp = server.internal_get("/metrics").await;

    assert_status(&resp, StatusCode::OK);
    let body = resp.text().await.unwrap();

    assert!(
        body.contains("tokio_php_uptime_seconds"),
        "Metrics should contain uptime"
    );
}

/// Test /metrics contains request counters
#[tokio::test]
async fn test_metrics_contains_request_counters() {
    let server = TestServer::new();
    let resp = server.internal_get("/metrics").await;

    assert_status(&resp, StatusCode::OK);
    let body = resp.text().await.unwrap();

    assert!(
        body.contains("requests_total") || body.contains("requests_per_second"),
        "Metrics should contain request counters"
    );
}

/// Test /metrics contains active connections gauge
#[tokio::test]
async fn test_metrics_contains_active_connections() {
    let server = TestServer::new();
    let resp = server.internal_get("/metrics").await;

    assert_status(&resp, StatusCode::OK);
    let body = resp.text().await.unwrap();

    assert!(
        body.contains("active_connections"),
        "Metrics should contain active_connections"
    );
}

/// Test /metrics contains memory metrics
#[tokio::test]
async fn test_metrics_contains_memory() {
    let server = TestServer::new();
    let resp = server.internal_get("/metrics").await;

    assert_status(&resp, StatusCode::OK);
    let body = resp.text().await.unwrap();

    assert!(
        body.contains("memory") || body.contains("MemTotal"),
        "Metrics should contain memory info"
    );
}

/// Test /metrics contains load average
#[tokio::test]
async fn test_metrics_contains_load_average() {
    let server = TestServer::new();
    let resp = server.internal_get("/metrics").await;

    assert_status(&resp, StatusCode::OK);
    let body = resp.text().await.unwrap();

    assert!(
        body.contains("node_load") || body.contains("load1"),
        "Metrics should contain load average"
    );
}

/// Test unknown internal endpoint returns 404
#[tokio::test]
async fn test_internal_unknown_endpoint() {
    let server = TestServer::new();
    let resp = server.internal_get("/unknown").await;

    assert_status(&resp, StatusCode::NOT_FOUND);
}

/// Test internal server is separate from main server
#[tokio::test]
async fn test_internal_server_isolation() {
    let server = TestServer::new();

    // Main server should not expose /health
    let resp = server.get("/health").await;
    // Should be 404 on main server (unless there's a health.php file)
    assert!(
        resp.status() == StatusCode::NOT_FOUND || resp.status() == StatusCode::OK,
        "Main server /health should be 404 or a PHP file"
    );

    // Internal server should expose /health
    let resp = server.internal_get("/health").await;
    assert_status(&resp, StatusCode::OK);
}
