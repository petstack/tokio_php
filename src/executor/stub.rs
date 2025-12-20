use async_trait::async_trait;

use super::{ExecutorError, ScriptExecutor};
use crate::types::{ScriptRequest, ScriptResponse};

/// Stub executor that returns empty responses.
///
/// Optimized for maximum throughput - returns pre-allocated empty response.
pub struct StubExecutor;

impl StubExecutor {
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

impl Default for StubExecutor {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl ScriptExecutor for StubExecutor {
    #[inline]
    async fn execute(&self, _request: ScriptRequest) -> Result<ScriptResponse, ExecutorError> {
        Ok(ScriptResponse::default())
    }

    #[inline]
    fn name(&self) -> &'static str {
        "stub"
    }

    #[inline]
    fn skip_file_check(&self) -> bool {
        true
    }

    #[inline]
    async fn execute_empty(&self) -> Result<ScriptResponse, ExecutorError> {
        Ok(ScriptResponse::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stub_returns_empty_body() {
        let executor = StubExecutor::new();

        let response = executor.execute_empty().await.unwrap();

        assert!(response.body.is_empty());
        assert!(response.headers.is_empty());
    }
}
