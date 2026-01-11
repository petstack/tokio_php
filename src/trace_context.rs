//! W3C Trace Context support for distributed tracing.
//!
//! Implements the W3C Trace Context specification:
//! <https://www.w3.org/TR/trace-context/>
//!
//! Format: `traceparent: {version}-{trace-id}-{parent-id}-{trace-flags}`
//! Example: `traceparent: 00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01`

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// W3C Trace Context version (always 00 for current spec).
const TRACE_VERSION: &str = "00";

/// Trace flags: sampled (01) or not sampled (00).
const FLAG_SAMPLED: u8 = 0x01;

/// Trace context containing trace ID, span ID, and flags.
#[derive(Debug, Clone)]
pub struct TraceContext {
    /// 16-byte trace ID as 32 hex chars
    pub trace_id: String,
    /// 8-byte span ID as 16 hex chars (this request's span)
    pub span_id: String,
    /// 8-byte parent span ID as 16 hex chars (from incoming request)
    pub parent_span_id: Option<String>,
    /// Trace flags (bit 0 = sampled)
    pub flags: u8,
}

impl TraceContext {
    /// Generate a new trace context (no parent).
    pub fn new() -> Self {
        Self {
            trace_id: generate_trace_id(),
            span_id: generate_span_id(),
            parent_span_id: None,
            flags: FLAG_SAMPLED,
        }
    }

    /// Create a child span from an existing trace context.
    pub fn child_from(parent: &TraceContext) -> Self {
        Self {
            trace_id: parent.trace_id.clone(),
            span_id: generate_span_id(),
            parent_span_id: Some(parent.span_id.clone()),
            flags: parent.flags,
        }
    }

    /// Parse from W3C traceparent header.
    ///
    /// Format: `{version}-{trace-id}-{parent-id}-{trace-flags}`
    /// Example: `00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01`
    pub fn parse(header: &str) -> Option<Self> {
        let parts: Vec<&str> = header.trim().split('-').collect();
        if parts.len() != 4 {
            return None;
        }

        let version = parts[0];
        let trace_id = parts[1];
        let parent_id = parts[2];
        let flags_str = parts[3];

        // Validate version (must be 00 for current spec)
        if version != "00" {
            return None;
        }

        // Validate trace-id (32 hex chars, not all zeros)
        if trace_id.len() != 32 || !is_valid_hex(trace_id) || is_all_zeros(trace_id) {
            return None;
        }

        // Validate parent-id (16 hex chars, not all zeros)
        if parent_id.len() != 16 || !is_valid_hex(parent_id) || is_all_zeros(parent_id) {
            return None;
        }

        // Validate flags (2 hex chars)
        if flags_str.len() != 2 || !is_valid_hex(flags_str) {
            return None;
        }

        let flags = u8::from_str_radix(flags_str, 16).ok()?;

        // Create new span ID for this request, parent is the incoming span
        Some(Self {
            trace_id: trace_id.to_lowercase(),
            span_id: generate_span_id(),
            parent_span_id: Some(parent_id.to_lowercase()),
            flags,
        })
    }

    /// Extract trace context from request headers, or generate new one.
    pub fn from_headers(headers: &hyper::HeaderMap) -> Self {
        headers
            .get("traceparent")
            .and_then(|v| v.to_str().ok())
            .and_then(Self::parse)
            .unwrap_or_else(Self::new)
    }

    /// Format as W3C traceparent header value.
    pub fn to_traceparent(&self) -> String {
        format!(
            "{}-{}-{}-{:02x}",
            TRACE_VERSION, self.trace_id, self.span_id, self.flags
        )
    }

    /// Check if trace is sampled.
    #[inline]
    pub fn is_sampled(&self) -> bool {
        self.flags & FLAG_SAMPLED != 0
    }
}

impl Default for TraceContext {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TraceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_traceparent())
    }
}

// =============================================================================
// ID Generation
// =============================================================================

/// Counter for unique span IDs within the same millisecond.
static SPAN_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a 32-character hex trace ID.
///
/// Format: timestamp (12 chars) + random (20 chars)
fn generate_trace_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;

    // Use timestamp + counter for uniqueness
    let counter = SPAN_COUNTER.fetch_add(1, Ordering::Relaxed);

    // Mix timestamp and counter with simple hash
    let high = ts ^ (counter.wrapping_mul(0x517cc1b727220a95));
    let low = counter ^ (ts.wrapping_mul(0x2545f4914f6cdd1d));

    format!("{:016x}{:016x}", high, low)
}

/// Generate a 16-character hex span ID.
fn generate_span_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    let counter = SPAN_COUNTER.fetch_add(1, Ordering::Relaxed);

    // Mix timestamp and counter
    let id = ts ^ counter.wrapping_mul(0x9e3779b97f4a7c15);

    format!("{:016x}", id)
}

// =============================================================================
// Validation Helpers
// =============================================================================

/// Check if string contains only valid hex characters.
#[inline]
fn is_valid_hex(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Check if string is all zeros.
#[inline]
fn is_all_zeros(s: &str) -> bool {
    s.chars().all(|c| c == '0')
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_traceparent() {
        let ctx = TraceContext::parse("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01");
        assert!(ctx.is_some());

        let ctx = ctx.unwrap();
        assert_eq!(ctx.trace_id, "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(ctx.parent_span_id, Some("b7ad6b7169203331".to_string()));
        assert!(ctx.is_sampled());
    }

    #[test]
    fn test_parse_invalid_traceparent() {
        // Wrong version
        assert!(TraceContext::parse("01-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01").is_none());

        // All zeros trace-id
        assert!(TraceContext::parse("00-00000000000000000000000000000000-b7ad6b7169203331-01").is_none());

        // All zeros parent-id
        assert!(TraceContext::parse("00-0af7651916cd43dd8448eb211c80319c-0000000000000000-01").is_none());

        // Wrong length
        assert!(TraceContext::parse("00-0af7651916cd43dd-b7ad6b7169203331-01").is_none());

        // Invalid hex
        assert!(TraceContext::parse("00-zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz-b7ad6b7169203331-01").is_none());
    }

    #[test]
    fn test_generate_new_context() {
        let ctx = TraceContext::new();
        assert_eq!(ctx.trace_id.len(), 32);
        assert_eq!(ctx.span_id.len(), 16);
        assert!(ctx.parent_span_id.is_none());
        assert!(ctx.is_sampled());
    }

    #[test]
    fn test_to_traceparent() {
        let ctx = TraceContext::parse("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01").unwrap();
        let header = ctx.to_traceparent();

        // trace_id stays the same, span_id is new
        assert!(header.starts_with("00-0af7651916cd43dd8448eb211c80319c-"));
        assert!(header.ends_with("-01"));
    }

    #[test]
    fn test_unique_span_ids() {
        let id1 = generate_span_id();
        let id2 = generate_span_id();
        let id3 = generate_span_id();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }
}
