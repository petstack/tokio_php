//! SSE (Server-Sent Events) streaming tests
//!
//! These tests verify that SSE streaming works correctly, including:
//! - Correct headers (Content-Type: text/event-stream)
//! - Events are streamed incrementally (not buffered until script end)
//! - Multiple concurrent SSE connections work
//! - flush() correctly sends data to client

use crate::helpers::*;
use reqwest::StatusCode;

/// Test SSE response headers are correct
#[tokio::test]
async fn test_sse_headers() {
    let server = TestServer::new();
    let resp = server
        .get_with_headers("/test_sse_minimal.php", &[("Accept", "text/event-stream")])
        .await;

    assert_status(&resp, StatusCode::OK);
    assert_header_starts_with(&resp, "content-type", "text/event-stream");
    assert_has_header(&resp, "x-request-id");
}

/// Test basic SSE events are received (minimal script without delays)
#[tokio::test]
async fn test_sse_minimal_events() {
    let server = TestServer::new();
    let events = server.collect_sse_events("/test_sse_minimal.php", 5).await;

    assert!(
        events.len() >= 3,
        "Expected at least 3 events, got {}",
        events.len()
    );
    assert!(
        events[0].data.contains("chunk1"),
        "First event should be chunk1"
    );
    assert!(
        events[1].data.contains("chunk2"),
        "Second event should be chunk2"
    );
    assert!(
        events[2].data.contains("chunk3"),
        "Third event should be chunk3"
    );
}

/// Test SSE streaming with sleep() - events should arrive incrementally
/// This is the key test that verifies the flush() fix works correctly
#[tokio::test]
async fn test_sse_streaming_with_delay() {
    let server = TestServer::new();

    // test_sse_timed.php sends 3 events with 500ms delay between each
    let (events, timestamps) = server
        .test_sse_streaming_timing("/test_sse_timed.php?count=3&delay=500", 500, 5)
        .await;

    assert!(
        events.len() >= 3,
        "Expected at least 3 events, got {}. Events: {:?}",
        events.len(),
        events
    );

    // Verify timestamps show incremental streaming (not all at once)
    if timestamps.len() >= 3 {
        // First event should arrive quickly (< 1 second)
        assert!(
            timestamps[0] < 1000,
            "First event should arrive quickly, but took {}ms",
            timestamps[0]
        );

        // Second event should arrive after ~500ms delay
        let gap1 = timestamps[1] - timestamps[0];
        assert!(
            gap1 >= 400 && gap1 < 1500,
            "Gap between event 0 and 1 should be ~500ms, got {}ms",
            gap1
        );

        // Third event should arrive after another ~500ms delay
        let gap2 = timestamps[2] - timestamps[1];
        assert!(
            gap2 >= 400 && gap2 < 1500,
            "Gap between event 1 and 2 should be ~500ms, got {}ms",
            gap2
        );
    }
}

/// Test SSE with 1 second delay (matches test_sse.php behavior)
#[tokio::test]
async fn test_sse_one_second_delay() {
    let server = TestServer::new();

    // Collect events for 3.5 seconds, should get at least 3 events
    let (events, timestamps) = server
        .test_sse_streaming_timing("/test_sse_timed.php?count=5&delay=1000", 1000, 4)
        .await;

    assert!(
        events.len() >= 3,
        "Expected at least 3 events in 3.5s with 1s delay, got {}",
        events.len()
    );

    // Verify streaming timing
    if timestamps.len() >= 2 {
        let gap = timestamps[1] - timestamps[0];
        assert!(
            gap >= 800 && gap < 2000,
            "Events should arrive ~1s apart, gap was {}ms",
            gap
        );
    }
}

/// Test concurrent SSE connections
#[tokio::test]
async fn test_sse_concurrent_connections() {
    let server = TestServer::new();

    // Spawn 5 concurrent SSE connections
    let mut handles = Vec::new();
    for _ in 0..5 {
        let client = server.client.clone();
        let url = format!("{}/test_sse_minimal.php", server.base_url);
        handles.push(tokio::spawn(async move {
            let resp = client
                .get(&url)
                .header("Accept", "text/event-stream")
                .send()
                .await;
            match resp {
                Ok(r) => r.text().await.ok(),
                Err(_) => None,
            }
        }));
    }

    // All connections should receive data
    let mut success_count = 0;
    for handle in handles {
        if let Ok(Some(body)) = handle.await {
            if body.contains("data:") && body.contains("chunk") {
                success_count += 1;
            }
        }
    }

    assert_eq!(
        success_count, 5,
        "All 5 concurrent SSE connections should receive data, got {}",
        success_count
    );
}

/// Test SSE data contains valid JSON
#[tokio::test]
async fn test_sse_json_data() {
    let server = TestServer::new();
    let events = server
        .collect_sse_events("/test_sse_timed.php?count=2&delay=100", 3)
        .await;

    assert!(!events.is_empty(), "Should receive at least 1 event");

    // Parse JSON in first event
    let first_data = &events[0].data;
    let json: Result<serde_json::Value, _> = serde_json::from_str(first_data);
    assert!(
        json.is_ok(),
        "SSE data should be valid JSON, got: {}",
        first_data
    );

    let json = json.unwrap();
    assert!(
        json.get("event").is_some(),
        "JSON should have 'event' field"
    );
    assert!(json.get("time").is_some(), "JSON should have 'time' field");
}

/// Test that non-SSE requests to SSE endpoint still work
#[tokio::test]
async fn test_sse_without_accept_header() {
    let server = TestServer::new();
    let resp = server.get("/test_sse_minimal.php").await;

    assert_status(&resp, StatusCode::OK);
    // Should still return SSE data even without Accept header
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("data:"),
        "Should still return SSE format data"
    );
}

/// Test SSE completion (full script execution)
#[tokio::test]
async fn test_sse_completion() {
    let server = TestServer::new();

    // Request all 3 events with 200ms delay, should complete in ~1 second
    let events = server
        .collect_sse_events("/test_sse_timed.php?count=3&delay=200", 3)
        .await;

    assert_eq!(events.len(), 3, "Should receive exactly 3 events");

    // Check event numbering
    for (i, event) in events.iter().enumerate() {
        let json: serde_json::Value = serde_json::from_str(&event.data).unwrap();
        let event_num = json["event"].as_u64().unwrap();
        assert_eq!(
            event_num,
            (i + 1) as u64,
            "Event {} should have number {}",
            i,
            i + 1
        );
    }
}

/// Test SSE with usleep (microsecond delays)
#[tokio::test]
async fn test_sse_usleep() {
    let server = TestServer::new();

    // 100ms delay between events
    let (events, timestamps) = server
        .test_sse_streaming_timing("/test_sse_timed.php?count=5&delay=100", 100, 3)
        .await;

    assert!(
        events.len() >= 4,
        "Expected at least 4 events with 100ms delay in 3s, got {}",
        events.len()
    );

    // With 100ms delays, events should still be streamed incrementally
    if timestamps.len() >= 2 {
        let total_time = timestamps.last().unwrap() - timestamps.first().unwrap();
        assert!(
            total_time >= 300,
            "Total streaming time should be >= 300ms for 4+ events with 100ms delay, got {}ms",
            total_time
        );
    }
}
