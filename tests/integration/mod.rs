//! Integration tests for tokio_php
//!
//! These tests require a running tokio_php server.
//! Run with: docker compose up -d && cargo test --test integration
//!
//! Environment variables:
//! - TEST_SERVER_URL: Base URL of the server (default: http://localhost:8081)
//! - TEST_INTERNAL_URL: Internal server URL (default: http://localhost:9091)

mod helpers;

mod compression;
mod error_pages;
mod http_basic;
mod internal_server;
mod php_execution;
mod rate_limiting;
mod sse;
mod static_files;
