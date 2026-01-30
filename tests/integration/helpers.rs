//! Test helpers and utilities

use reqwest::{Client, Response, StatusCode};
use std::time::Duration;

/// Test server configuration
pub struct TestServer {
    pub base_url: String,
    pub internal_url: String,
    pub client: Client,
}

#[allow(dead_code)]
impl TestServer {
    /// Create a new TestServer with default or env-configured URLs
    pub fn new() -> Self {
        let base_url = std::env::var("TEST_SERVER_URL")
            .unwrap_or_else(|_| "http://localhost:8081".to_string());
        let internal_url = std::env::var("TEST_INTERNAL_URL")
            .unwrap_or_else(|_| "http://localhost:9091".to_string());

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            base_url,
            internal_url,
            client,
        }
    }

    /// Make a GET request to the server
    pub async fn get(&self, path: &str) -> Response {
        self.client
            .get(format!("{}{}", self.base_url, path))
            .send()
            .await
            .expect("GET request failed")
    }

    /// Make a GET request with custom headers
    pub async fn get_with_headers(&self, path: &str, headers: &[(&str, &str)]) -> Response {
        let mut req = self.client.get(format!("{}{}", self.base_url, path));
        for (name, value) in headers {
            req = req.header(*name, *value);
        }
        req.send().await.expect("GET request failed")
    }

    /// Make a POST request with form data
    pub async fn post_form(&self, path: &str, form: &[(&str, &str)]) -> Response {
        self.client
            .post(format!("{}{}", self.base_url, path))
            .form(form)
            .send()
            .await
            .expect("POST request failed")
    }

    /// Make a POST request with JSON body
    pub async fn post_json<T: serde::Serialize + ?Sized>(&self, path: &str, json: &T) -> Response {
        self.client
            .post(format!("{}{}", self.base_url, path))
            .json(json)
            .send()
            .await
            .expect("POST request failed")
    }

    /// Make a request to the internal server
    pub async fn internal_get(&self, path: &str) -> Response {
        self.client
            .get(format!("{}{}", self.internal_url, path))
            .send()
            .await
            .expect("Internal GET request failed")
    }

    /// Check if server is running
    pub async fn is_running(&self) -> bool {
        match self
            .client
            .get(format!("{}/health", self.internal_url))
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            Ok(resp) => resp.status() == StatusCode::OK,
            Err(_) => false,
        }
    }

    /// Wait for server to be ready
    pub async fn wait_for_ready(&self, timeout: Duration) {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if self.is_running().await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!(
            "Server not ready after {:?}. Make sure docker compose is running.",
            timeout
        );
    }
}

impl Default for TestServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Assert that response has expected status
pub fn assert_status(response: &Response, expected: StatusCode) {
    assert_eq!(
        response.status(),
        expected,
        "Expected status {}, got {}",
        expected,
        response.status()
    );
}

/// Assert that response contains header
pub fn assert_header(response: &Response, name: &str, expected: &str) {
    let value = response
        .headers()
        .get(name)
        .unwrap_or_else(|| panic!("Header '{}' not found", name))
        .to_str()
        .unwrap();
    assert_eq!(value, expected, "Header '{}' mismatch", name);
}

/// Assert that response contains header with prefix
pub fn assert_header_starts_with(response: &Response, name: &str, prefix: &str) {
    let value = response
        .headers()
        .get(name)
        .unwrap_or_else(|| panic!("Header '{}' not found", name))
        .to_str()
        .unwrap();
    assert!(
        value.starts_with(prefix),
        "Header '{}' expected to start with '{}', got '{}'",
        name,
        prefix,
        value
    );
}

/// Assert that response has header present
pub fn assert_has_header(response: &Response, name: &str) {
    assert!(
        response.headers().contains_key(name),
        "Header '{}' not found",
        name
    );
}

/// Assert that response body contains substring
pub async fn assert_body_contains(response: Response, substring: &str) {
    let body = response.text().await.expect("Failed to read body");
    assert!(
        body.contains(substring),
        "Body does not contain '{}'. Body: {}",
        substring,
        &body[..body.len().min(500)]
    );
}

// =============================================================================
// SSE (Server-Sent Events) Testing Helpers
// =============================================================================

/// SSE event parsed from stream
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
    pub id: Option<String>,
}

impl SseEvent {
    /// Parse SSE events from a raw string
    pub fn parse_all(input: &str) -> Vec<SseEvent> {
        let mut events = Vec::new();
        let mut current_event: Option<String> = None;
        let mut current_data = String::new();
        let mut current_id: Option<String> = None;

        for line in input.lines() {
            if line.is_empty() {
                // Empty line = end of event
                if !current_data.is_empty() {
                    events.push(SseEvent {
                        event: current_event.take(),
                        data: std::mem::take(&mut current_data),
                        id: current_id.take(),
                    });
                }
            } else if let Some(data) = line.strip_prefix("data: ") {
                if !current_data.is_empty() {
                    current_data.push('\n');
                }
                current_data.push_str(data);
            } else if let Some(data) = line.strip_prefix("data:") {
                if !current_data.is_empty() {
                    current_data.push('\n');
                }
                current_data.push_str(data);
            } else if let Some(event) = line.strip_prefix("event: ") {
                current_event = Some(event.to_string());
            } else if let Some(id) = line.strip_prefix("id: ") {
                current_id = Some(id.to_string());
            }
        }

        // Handle case where stream doesn't end with empty line
        if !current_data.is_empty() {
            events.push(SseEvent {
                event: current_event,
                data: current_data,
                id: current_id,
            });
        }

        events
    }
}

impl TestServer {
    /// Make a streaming GET request with timeout
    /// Returns the accumulated body up to timeout or connection close
    pub async fn get_streaming(&self, path: &str, timeout_secs: u64) -> String {
        use futures_util::StreamExt;
        use tokio::time::timeout;

        let resp = self
            .client
            .get(format!("{}{}", self.base_url, path))
            .header("Accept", "text/event-stream")
            .send()
            .await
            .expect("Streaming GET request failed");

        let mut body = String::new();
        let mut stream = resp.bytes_stream();

        let _ = timeout(Duration::from_secs(timeout_secs), async {
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                            body.push_str(&text);
                        }
                    }
                    Err(_) => break,
                }
            }
        })
        .await;

        body
    }

    /// Collect SSE events for a given duration
    pub async fn collect_sse_events(&self, path: &str, timeout_secs: u64) -> Vec<SseEvent> {
        let body = self.get_streaming(path, timeout_secs).await;
        SseEvent::parse_all(&body)
    }

    /// Test that SSE events are received incrementally (streaming works)
    /// Returns (events_received, time_elapsed_ms)
    pub async fn test_sse_streaming_timing(
        &self,
        path: &str,
        expected_delay_ms: u64,
        timeout_secs: u64,
    ) -> (Vec<SseEvent>, Vec<u128>) {
        use futures_util::StreamExt;
        use tokio::time::{timeout, Instant};

        let resp = self
            .client
            .get(format!("{}{}", self.base_url, path))
            .header("Accept", "text/event-stream")
            .send()
            .await
            .expect("Streaming GET request failed");

        let mut events = Vec::new();
        let mut timestamps = Vec::new();
        let mut current_data = String::new();
        let mut stream = resp.bytes_stream();
        let start = Instant::now();

        let _ = timeout(Duration::from_secs(timeout_secs), async {
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                            current_data.push_str(&text);

                            // Parse any complete events
                            while let Some(idx) = current_data.find("\n\n") {
                                let event_str = &current_data[..idx];
                                if let Some(data) = event_str.strip_prefix("data: ") {
                                    events.push(SseEvent {
                                        event: None,
                                        data: data.to_string(),
                                        id: None,
                                    });
                                    timestamps.push(start.elapsed().as_millis());
                                } else if event_str.starts_with("data:") {
                                    let data = event_str.trim_start_matches("data:").trim();
                                    events.push(SseEvent {
                                        event: None,
                                        data: data.to_string(),
                                        id: None,
                                    });
                                    timestamps.push(start.elapsed().as_millis());
                                }
                                current_data = current_data[idx + 2..].to_string();
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        })
        .await;

        // Verify streaming behavior by checking timestamps
        if expected_delay_ms > 0 && timestamps.len() >= 2 {
            for i in 1..timestamps.len() {
                let gap = timestamps[i] - timestamps[i - 1];
                // Allow some tolerance (50% of expected delay)
                let min_expected = expected_delay_ms as u128 / 2;
                assert!(
                    gap >= min_expected,
                    "Events should be streamed with delay. Gap between event {} and {}: {}ms (expected >= {}ms)",
                    i - 1,
                    i,
                    gap,
                    min_expected
                );
            }
        }

        (events, timestamps)
    }
}
