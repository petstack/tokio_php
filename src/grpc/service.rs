//! gRPC service implementation.

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use crate::executor::ScriptExecutor;
use crate::health::{HealthChecker, ProbeType};

use super::conversion;
use super::proto::health_check_response::ServingStatus;
use super::proto::php_service_server::PhpService;
use super::proto::{
    ExecuteRequest, ExecuteResponse, HealthCheckRequest, HealthCheckResponse,
    StreamChunk as GrpcStreamChunk,
};

/// PHP service implementation for gRPC.
pub struct PhpServiceImpl<E: ScriptExecutor> {
    executor: Arc<E>,
    health_checker: Arc<HealthChecker>,
    document_root: String,
}

impl<E: ScriptExecutor> PhpServiceImpl<E> {
    /// Create a new PHP service.
    pub fn new(
        executor: Arc<E>,
        health_checker: Arc<HealthChecker>,
        document_root: String,
    ) -> Self {
        Self {
            executor,
            health_checker,
            document_root,
        }
    }
}

#[tonic::async_trait]
impl<E: ScriptExecutor + 'static> PhpService for PhpServiceImpl<E> {
    /// Execute a PHP script (unary RPC).
    async fn execute(
        &self,
        request: Request<ExecuteRequest>,
    ) -> Result<Response<ExecuteResponse>, Status> {
        let req = request.into_inner();

        // Convert gRPC request to ScriptRequest
        let script_request = conversion::grpc_to_script_request(&req, &self.document_root)
            .map_err(Status::invalid_argument)?;

        // Generate request ID
        let request_id = format!(
            "{:08x}{:04x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                & 0xFFFFFFFF,
            simple_random_u16()
        );

        // Execute via standard executor
        let start = Instant::now();
        let result = self
            .executor
            .execute(script_request)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        // Convert ScriptResponse to gRPC response
        let response = conversion::script_response_to_grpc(result, start.elapsed(), request_id);

        Ok(Response::new(response))
    }

    type ExecuteStreamStream = ReceiverStream<Result<GrpcStreamChunk, Status>>;

    /// Execute with server-streaming response (for SSE/long-polling).
    async fn execute_stream(
        &self,
        request: Request<ExecuteRequest>,
    ) -> Result<Response<Self::ExecuteStreamStream>, Status> {
        let req = request.into_inner();

        let script_request = conversion::grpc_to_script_request(&req, &self.document_root)
            .map_err(Status::invalid_argument)?;

        // Create channel for gRPC streaming
        let (tx, rx) = mpsc::channel(100);

        // Execute with streaming
        let executor = Arc::clone(&self.executor);
        tokio::spawn(async move {
            match executor.execute_streaming(script_request, 100).await {
                Ok(mut stream_rx) => {
                    let mut sequence = 0;
                    while let Some(chunk) = stream_rx.recv().await {
                        let is_final = chunk.is_empty();
                        let grpc_chunk = GrpcStreamChunk {
                            data: chunk.data.to_vec(),
                            is_final,
                            sequence,
                        };
                        sequence += 1;
                        if tx.send(Ok(grpc_chunk)).await.is_err() {
                            break;
                        }
                        if is_final {
                            break;
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(Status::internal(e.to_string()))).await;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    /// gRPC health checking protocol.
    async fn check(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        let status = self.health_checker.check(ProbeType::Readiness);

        let serving_status = if status.is_healthy() {
            ServingStatus::Serving
        } else {
            ServingStatus::NotServing
        };

        Ok(Response::new(HealthCheckResponse {
            status: serving_status as i32,
        }))
    }
}

/// Simple random u16 generation without external crate.
fn simple_random_u16() -> u16 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos & 0xFFFF) as u16
}
