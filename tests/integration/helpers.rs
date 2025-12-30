//! Test helpers and utilities

use reqwest::{Client, Response, StatusCode};
use std::time::Duration;

/// Test server configuration
pub struct TestServer {
    pub base_url: String,
    pub internal_url: String,
    pub client: Client,
}

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
    pub async fn get_with_headers(
        &self,
        path: &str,
        headers: &[(&str, &str)],
    ) -> Response {
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
    pub async fn post_json<T: serde::Serialize + ?Sized>(
        &self,
        path: &str,
        json: &T,
    ) -> Response {
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
        match self.client
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
