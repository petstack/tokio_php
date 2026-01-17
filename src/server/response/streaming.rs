//! Streaming response support for SSE and chunked transfer.
//!
//! This module provides types and utilities for building streaming HTTP responses,
//! enabling Server-Sent Events (SSE) and chunked transfer encoding.
//!
//! # Example
//!
//! ```rust,ignore
//! use tokio::sync::mpsc;
//! use crate::server::response::streaming::{StreamChunk, streaming_response};
//!
//! let (tx, rx) = mpsc::channel(100);
//!
//! // Send chunks from another task
//! tokio::spawn(async move {
//!     tx.send(StreamChunk::new("data: hello\n\n")).await.ok();
//!     tx.send(StreamChunk::new("data: world\n\n")).await.ok();
//! });
//!
//! // Build streaming response
//! let response = streaming_response(200, headers, rx);
//! ```

use bytes::Bytes;
use http_body_util::StreamBody;
use hyper::body::Frame;
use hyper::Response;
use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;

/// A chunk of streaming data.
#[derive(Debug, Clone)]
pub struct StreamChunk {
    /// The data bytes for this chunk.
    pub data: Bytes,
}

impl StreamChunk {
    /// Create a new stream chunk from bytes.
    #[inline]
    pub fn new(data: impl Into<Bytes>) -> Self {
        Self { data: data.into() }
    }

    /// Create an empty chunk (used for keep-alive).
    #[inline]
    pub fn empty() -> Self {
        Self { data: Bytes::new() }
    }

    /// Check if this chunk is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl From<Bytes> for StreamChunk {
    fn from(data: Bytes) -> Self {
        Self { data }
    }
}

impl From<Vec<u8>> for StreamChunk {
    fn from(data: Vec<u8>) -> Self {
        Self {
            data: Bytes::from(data),
        }
    }
}

impl From<&[u8]> for StreamChunk {
    fn from(data: &[u8]) -> Self {
        Self {
            data: Bytes::copy_from_slice(data),
        }
    }
}

impl From<String> for StreamChunk {
    fn from(data: String) -> Self {
        Self {
            data: Bytes::from(data),
        }
    }
}

impl From<&str> for StreamChunk {
    fn from(data: &str) -> Self {
        Self {
            data: Bytes::copy_from_slice(data.as_bytes()),
        }
    }
}

/// Wrapper stream that converts `StreamChunk` to `Frame<Bytes>`.
pub struct ChunkFrameStream {
    inner: ReceiverStream<StreamChunk>,
}

impl ChunkFrameStream {
    /// Create a new chunk frame stream from a receiver.
    pub fn new(rx: mpsc::Receiver<StreamChunk>) -> Self {
        Self {
            inner: ReceiverStream::new(rx),
        }
    }
}

impl Stream for ChunkFrameStream {
    type Item = Result<Frame<Bytes>, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(chunk)) => {
                // Skip empty chunks (or use them as comments for keep-alive)
                if chunk.is_empty() {
                    // SSE comment for keep-alive
                    Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(
                        b": keepalive\n\n",
                    )))))
                } else {
                    Poll::Ready(Some(Ok(Frame::data(chunk.data))))
                }
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Type alias for streaming body using our chunk stream.
pub type StreamingBody = StreamBody<ChunkFrameStream>;

/// Type alias for streaming HTTP response.
pub type StreamingResponse = Response<StreamingBody>;

/// Create a streaming response from a receiver channel.
///
/// The response will use chunked transfer encoding and stream data
/// as it becomes available from the channel.
///
/// # Arguments
///
/// * `status` - HTTP status code
/// * `headers` - Response headers (name, value pairs)
/// * `body_rx` - Channel receiver for streaming chunks
///
/// # Returns
///
/// A streaming HTTP response that sends chunks as they arrive.
pub fn streaming_response(
    status: u16,
    headers: Vec<(String, String)>,
    body_rx: mpsc::Receiver<StreamChunk>,
) -> StreamingResponse {
    let frame_stream = ChunkFrameStream::new(body_rx);
    let body = StreamBody::new(frame_stream);

    let mut builder = Response::builder().status(status);

    for (name, value) in headers {
        builder = builder.header(name, value);
    }

    builder.body(body).unwrap()
}

/// Create a streaming SSE response with default headers.
///
/// Sets the following headers automatically:
/// - `Content-Type: text/event-stream`
/// - `Cache-Control: no-cache`
/// - `Connection: keep-alive`
/// - `X-Accel-Buffering: no` (for nginx compatibility)
///
/// # Arguments
///
/// * `body_rx` - Channel receiver for streaming chunks
/// * `extra_headers` - Additional headers to include
///
/// # Returns
///
/// A streaming SSE response.
pub fn sse_response(
    body_rx: mpsc::Receiver<StreamChunk>,
    extra_headers: Vec<(String, String)>,
) -> StreamingResponse {
    let mut headers = vec![
        ("Content-Type".to_string(), "text/event-stream".to_string()),
        ("Cache-Control".to_string(), "no-cache".to_string()),
        ("Connection".to_string(), "keep-alive".to_string()),
        ("X-Accel-Buffering".to_string(), "no".to_string()),
    ];

    headers.extend(extra_headers);

    streaming_response(200, headers, body_rx)
}

/// Check if the Accept header indicates an SSE request.
///
/// Returns `true` if the Accept header contains `text/event-stream`.
#[inline]
pub fn is_sse_accept(accept: Option<&str>) -> bool {
    accept
        .map(|a| a.contains("text/event-stream"))
        .unwrap_or(false)
}

/// Check if Content-Type indicates an SSE response.
#[inline]
pub fn is_sse_content_type(content_type: Option<&str>) -> bool {
    content_type
        .map(|ct| ct.contains("text/event-stream"))
        .unwrap_or(false)
}

/// Default buffer size for streaming channels.
pub const DEFAULT_STREAM_BUFFER_SIZE: usize = 100;

/// Create a new streaming channel pair.
///
/// Returns a sender for sending chunks and a receiver for the response.
#[inline]
pub fn stream_channel(
    buffer_size: usize,
) -> (mpsc::Sender<StreamChunk>, mpsc::Receiver<StreamChunk>) {
    mpsc::channel(buffer_size)
}
