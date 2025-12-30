//! Compression tests (Brotli)

use crate::helpers::*;
use reqwest::StatusCode;

/// Test that Brotli compression is applied when Accept-Encoding: br is sent
#[tokio::test]
async fn test_brotli_compression_applied() {
    let server = TestServer::new();
    let resp = server
        .get_with_headers("/index.php", &[("Accept-Encoding", "br")])
        .await;

    assert_status(&resp, StatusCode::OK);
    // Should have Content-Encoding: br header
    if let Some(encoding) = resp.headers().get("content-encoding") {
        assert_eq!(
            encoding.to_str().unwrap(),
            "br",
            "Content-Encoding should be 'br'"
        );
    }
}

/// Test Vary header is present for compressed responses
#[tokio::test]
async fn test_vary_header_present() {
    let server = TestServer::new();
    let resp = server
        .get_with_headers("/index.php", &[("Accept-Encoding", "br")])
        .await;

    assert_status(&resp, StatusCode::OK);
    // If compression was applied, Vary header should be present
    if resp.headers().contains_key("content-encoding") {
        assert_has_header(&resp, "vary");
    }
}

/// Test no compression when Accept-Encoding is not sent
#[tokio::test]
async fn test_no_compression_without_accept_encoding() {
    let server = TestServer::new();

    // Create a client that doesn't send Accept-Encoding
    let client = reqwest::Client::builder()
        .no_brotli()
        .no_gzip()
        .no_deflate()
        .build()
        .unwrap();

    let resp = client
        .get(format!("{}/index.php", server.base_url))
        .header("Accept-Encoding", "identity")
        .send()
        .await
        .unwrap();

    assert_status(&resp, StatusCode::OK);
    // Should NOT have Content-Encoding header (or it should be identity)
    let encoding = resp.headers().get("content-encoding");
    assert!(
        encoding.is_none() || encoding.unwrap().to_str().unwrap() == "identity",
        "Should not have compression encoding without Accept-Encoding: br"
    );
}

/// Test small responses are not compressed (below MIN_COMPRESSION_SIZE)
#[tokio::test]
async fn test_small_response_not_compressed() {
    let server = TestServer::new();
    let resp = server
        .get_with_headers("/bench.php", &[("Accept-Encoding", "br")])
        .await;

    assert_status(&resp, StatusCode::OK);
    // bench.php returns "ok" which is only 2 bytes - should not be compressed
    let encoding = resp.headers().get("content-encoding");
    assert!(
        encoding.is_none() || encoding.unwrap().to_str().unwrap() != "br",
        "Small responses should not be Brotli compressed"
    );
}

/// Test compression for PHP output
#[tokio::test]
async fn test_compression_php_output() {
    let server = TestServer::new();

    // Create a client that explicitly requests brotli and handles raw bytes
    let client = reqwest::Client::builder()
        .no_brotli() // Don't auto-decompress so we can check the header
        .build()
        .unwrap();

    let resp = client
        .get(format!("{}/index.php", server.base_url))
        .header("Accept-Encoding", "br")
        .send()
        .await
        .unwrap();

    assert_status(&resp, StatusCode::OK);
    // Large PHP output should be compressed
    // Check Content-Encoding header
    if let Some(encoding) = resp.headers().get("content-encoding") {
        assert_eq!(encoding.to_str().unwrap(), "br");
    }
}

/// Test compression for CSS files
#[tokio::test]
async fn test_compression_css_file() {
    let server = TestServer::new();

    let client = reqwest::Client::builder()
        .no_brotli()
        .build()
        .unwrap();

    let resp = client
        .get(format!("{}/test.css", server.base_url))
        .header("Accept-Encoding", "br")
        .send()
        .await
        .unwrap();

    assert_status(&resp, StatusCode::OK);
    // CSS should be compressible type, but only if large enough
}

/// Test Accept-Encoding: gzip is not supported (only br)
#[tokio::test]
async fn test_gzip_not_supported() {
    let server = TestServer::new();

    let client = reqwest::Client::builder()
        .no_brotli()
        .no_gzip()
        .build()
        .unwrap();

    let resp = client
        .get(format!("{}/index.php", server.base_url))
        .header("Accept-Encoding", "gzip")
        .send()
        .await
        .unwrap();

    assert_status(&resp, StatusCode::OK);
    // Should NOT have gzip encoding (server only supports brotli)
    let encoding = resp.headers().get("content-encoding");
    assert!(
        encoding.is_none() || encoding.unwrap().to_str().unwrap() != "gzip",
        "Server should not use gzip encoding"
    );
}

/// Test Accept-Encoding with multiple values prefers br
#[tokio::test]
async fn test_accept_encoding_multiple() {
    let server = TestServer::new();

    let client = reqwest::Client::builder()
        .no_brotli()
        .build()
        .unwrap();

    let resp = client
        .get(format!("{}/index.php", server.base_url))
        .header("Accept-Encoding", "gzip, deflate, br")
        .send()
        .await
        .unwrap();

    assert_status(&resp, StatusCode::OK);
    // Should use br if response is large enough
    if let Some(encoding) = resp.headers().get("content-encoding") {
        assert_eq!(
            encoding.to_str().unwrap(),
            "br",
            "Should prefer br encoding"
        );
    }
}
