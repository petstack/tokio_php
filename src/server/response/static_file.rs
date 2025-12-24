//! Static file serving.

use std::path::Path;

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};

use super::compression::{compress_brotli, should_compress_mime, MIN_COMPRESSION_SIZE};
use super::EMPTY_BODY;

/// Serve a static file from the filesystem.
pub async fn serve_static_file(file_path: &Path, use_brotli: bool) -> Response<Full<Bytes>> {
    match tokio::fs::read(file_path).await {
        Ok(contents) => {
            let mime = mime_guess::from_path(file_path)
                .first_or_octet_stream()
                .to_string();

            // Check if we should compress this file
            let should_compress =
                use_brotli && contents.len() >= MIN_COMPRESSION_SIZE && should_compress_mime(&mime);

            let (final_body, is_compressed) = if should_compress {
                if let Some(compressed) = compress_brotli(&contents) {
                    (Bytes::from(compressed), true)
                } else {
                    (Bytes::from(contents), false)
                }
            } else {
                (Bytes::from(contents), false)
            };

            let mut builder = Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", &mime)
                .header("Server", "tokio_php/0.1.0");

            if is_compressed {
                builder = builder
                    .header("Content-Encoding", "br")
                    .header("Vary", "Accept-Encoding");
            }

            builder.body(Full::new(final_body)).unwrap()
        }
        Err(e) => {
            tracing::error!("Failed to read file {:?}: {}", file_path, e);
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "text/plain")
                .body(Full::new(EMPTY_BODY.clone()))
                .unwrap()
        }
    }
}
