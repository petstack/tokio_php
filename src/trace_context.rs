//! W3C Trace Context support for distributed tracing.
//!
//! Implements the W3C Trace Context specification:
//! <https://www.w3.org/TR/trace-context/>
//!
//! Format: `traceparent: {version}-{trace-id}-{parent-id}-{trace-flags}`
//! Example: `traceparent: 00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01`
//!
//! This implementation uses stack-allocated buffers for zero heap allocation.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Trace flags: sampled (01) or not sampled (00).
const FLAG_SAMPLED: u8 = 0x01;

/// Hex lookup table for fast u8 -> hex conversion
const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

/// Trace context containing trace ID, span ID, and flags.
/// All fields are stack-allocated for zero heap allocation.
///
/// Memory layout (73 bytes total):
/// - trace_id: 32 bytes
/// - span_id: 16 bytes
/// - parent_span_id: 17 bytes (1 byte flag + 16 bytes data)
/// - flags: 1 byte
/// - traceparent_buf: 55 bytes (cached traceparent header)
/// - short_id_buf: 17 bytes (cached request ID)
#[derive(Clone, Copy)]
pub struct TraceContext {
    /// 16-byte trace ID as 32 hex chars
    trace_id: [u8; 32],
    /// 8-byte span ID as 16 hex chars (this request's span)
    span_id: [u8; 16],
    /// 8-byte parent span ID as 16 hex chars (from incoming request)
    /// First byte is 0 if None, 1 if Some
    parent_span_id: [u8; 17],
    /// Trace flags (bit 0 = sampled)
    flags: u8,
    /// Cached traceparent header value: "00-{trace_id}-{span_id}-{flags}"
    traceparent_buf: [u8; 55],
    /// Cached short ID for request correlation: "{trace_id[0:12]}-{span_id[0:4]}"
    short_id_buf: [u8; 17],
}

impl TraceContext {
    /// Generate a new trace context (no parent).
    #[inline]
    pub fn new() -> Self {
        let mut ctx = Self {
            trace_id: [0u8; 32],
            span_id: [0u8; 16],
            parent_span_id: [0u8; 17], // First byte 0 = None
            flags: FLAG_SAMPLED,
            traceparent_buf: [0u8; 55],
            short_id_buf: [0u8; 17],
        };

        generate_trace_id(&mut ctx.trace_id);
        generate_span_id(&mut ctx.span_id);
        ctx.build_cached_values();
        ctx
    }

    /// Parse from W3C traceparent header.
    ///
    /// Format: `{version}-{trace-id}-{parent-id}-{trace-flags}`
    /// Example: `00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01`
    pub fn parse(header: &str) -> Option<Self> {
        let bytes = header.trim().as_bytes();

        // Quick length check: "00-" + 32 + "-" + 16 + "-" + 2 = 55 chars minimum
        if bytes.len() < 55 {
            return None;
        }

        // Check version (must be "00")
        if bytes[0] != b'0' || bytes[1] != b'0' || bytes[2] != b'-' {
            return None;
        }

        // Parse trace_id (chars 3..35)
        let trace_id_slice = &bytes[3..35];
        if !is_valid_hex_bytes(trace_id_slice) || is_all_zeros_bytes(trace_id_slice) {
            return None;
        }

        // Check separator
        if bytes[35] != b'-' {
            return None;
        }

        // Parse parent_id (chars 36..52)
        let parent_id_slice = &bytes[36..52];
        if !is_valid_hex_bytes(parent_id_slice) || is_all_zeros_bytes(parent_id_slice) {
            return None;
        }

        // Check separator
        if bytes[52] != b'-' {
            return None;
        }

        // Parse flags (chars 53..55)
        let flags_slice = &bytes[53..55];
        if !is_valid_hex_bytes(flags_slice) {
            return None;
        }

        let flags = hex_byte_to_u8(flags_slice[0])? * 16 + hex_byte_to_u8(flags_slice[1])?;

        let mut ctx = Self {
            trace_id: [0u8; 32],
            span_id: [0u8; 16],
            parent_span_id: [0u8; 17],
            flags,
            traceparent_buf: [0u8; 55],
            short_id_buf: [0u8; 17],
        };

        // Copy and lowercase trace_id
        for (i, &b) in trace_id_slice.iter().enumerate() {
            ctx.trace_id[i] = b.to_ascii_lowercase();
        }

        // Generate new span_id for this request
        generate_span_id(&mut ctx.span_id);

        // Store parent_span_id (lowercase)
        ctx.parent_span_id[0] = 1; // Mark as Some
        for (i, &b) in parent_id_slice.iter().enumerate() {
            ctx.parent_span_id[i + 1] = b.to_ascii_lowercase();
        }

        ctx.build_cached_values();
        Some(ctx)
    }

    /// Extract trace context from request headers, or generate new one.
    #[inline]
    pub fn from_headers(headers: &hyper::HeaderMap) -> Self {
        headers
            .get("traceparent")
            .and_then(|v| v.to_str().ok())
            .and_then(Self::parse)
            .unwrap_or_else(Self::new)
    }

    /// Build cached traceparent and short_id values.
    #[inline]
    fn build_cached_values(&mut self) {
        // Build traceparent: "00-{trace_id}-{span_id}-{flags:02x}"
        self.traceparent_buf[0] = b'0';
        self.traceparent_buf[1] = b'0';
        self.traceparent_buf[2] = b'-';
        self.traceparent_buf[3..35].copy_from_slice(&self.trace_id);
        self.traceparent_buf[35] = b'-';
        self.traceparent_buf[36..52].copy_from_slice(&self.span_id);
        self.traceparent_buf[52] = b'-';
        self.traceparent_buf[53] = HEX_CHARS[(self.flags >> 4) as usize];
        self.traceparent_buf[54] = HEX_CHARS[(self.flags & 0x0f) as usize];

        // Build short_id: "{trace_id[0:12]}-{span_id[0:4]}"
        self.short_id_buf[..12].copy_from_slice(&self.trace_id[..12]);
        self.short_id_buf[12] = b'-';
        self.short_id_buf[13..17].copy_from_slice(&self.span_id[..4]);
    }

    /// Get trace ID as string slice (32 hex chars).
    #[inline]
    pub fn trace_id(&self) -> &str {
        // SAFETY: We only store ASCII hex digits
        unsafe { std::str::from_utf8_unchecked(&self.trace_id) }
    }

    /// Get span ID as string slice (16 hex chars).
    #[inline]
    pub fn span_id(&self) -> &str {
        // SAFETY: We only store ASCII hex digits
        unsafe { std::str::from_utf8_unchecked(&self.span_id) }
    }

    /// Get parent span ID as string slice (16 hex chars), if present.
    #[inline]
    pub fn parent_span_id(&self) -> Option<&str> {
        if self.parent_span_id[0] == 0 {
            None
        } else {
            // SAFETY: We only store ASCII hex digits
            Some(unsafe { std::str::from_utf8_unchecked(&self.parent_span_id[1..17]) })
        }
    }

    /// Get traceparent header value (55 chars).
    /// Format: "00-{trace_id}-{span_id}-{flags:02x}"
    #[inline]
    pub fn traceparent(&self) -> &str {
        // SAFETY: We only store ASCII chars
        unsafe { std::str::from_utf8_unchecked(&self.traceparent_buf) }
    }

    /// Get short ID for request correlation (17 chars).
    /// Format: "{trace_id[0:12]}-{span_id[0:4]}"
    #[inline]
    pub fn short_id(&self) -> &str {
        // SAFETY: We only store ASCII chars
        unsafe { std::str::from_utf8_unchecked(&self.short_id_buf) }
    }

    /// Check if trace is sampled.
    #[inline]
    pub fn is_sampled(&self) -> bool {
        self.flags & FLAG_SAMPLED != 0
    }

    /// Get flags value.
    #[inline]
    pub fn flags(&self) -> u8 {
        self.flags
    }
}

impl Default for TraceContext {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for TraceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TraceContext")
            .field("trace_id", &self.trace_id())
            .field("span_id", &self.span_id())
            .field("parent_span_id", &self.parent_span_id())
            .field("flags", &self.flags)
            .finish()
    }
}

impl fmt::Display for TraceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.traceparent())
    }
}

// =============================================================================
// ID Generation (zero-allocation)
// =============================================================================

/// Counter for unique span IDs within the same millisecond.
static SPAN_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a 32-character hex trace ID into buffer.
#[inline]
fn generate_trace_id(buf: &mut [u8; 32]) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;

    let counter = SPAN_COUNTER.fetch_add(1, Ordering::Relaxed);

    // Mix timestamp and counter with simple hash
    let high = ts ^ (counter.wrapping_mul(0x517cc1b727220a95));
    let low = counter ^ (ts.wrapping_mul(0x2545f4914f6cdd1d));

    u64_to_hex(high, &mut buf[0..16]);
    u64_to_hex(low, &mut buf[16..32]);
}

/// Generate a 16-character hex span ID into buffer.
#[inline]
fn generate_span_id(buf: &mut [u8; 16]) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    let counter = SPAN_COUNTER.fetch_add(1, Ordering::Relaxed);

    // Mix timestamp and counter
    let id = ts ^ counter.wrapping_mul(0x9e3779b97f4a7c15);

    u64_to_hex(id, buf);
}

/// Convert u64 to 16 hex characters.
#[inline]
fn u64_to_hex(val: u64, buf: &mut [u8]) {
    debug_assert!(buf.len() >= 16);
    for (i, byte) in buf.iter_mut().enumerate().take(16) {
        let nibble = ((val >> (60 - i * 4)) & 0x0f) as usize;
        *byte = HEX_CHARS[nibble];
    }
}

// =============================================================================
// Validation Helpers
// =============================================================================

/// Check if byte slice contains only valid hex characters.
#[inline]
fn is_valid_hex_bytes(s: &[u8]) -> bool {
    s.iter().all(|&b| b.is_ascii_hexdigit())
}

/// Check if byte slice is all ASCII zeros.
#[inline]
fn is_all_zeros_bytes(s: &[u8]) -> bool {
    s.iter().all(|&b| b == b'0')
}

/// Convert hex char byte to u8 value.
#[inline]
fn hex_byte_to_u8(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_traceparent() {
        let ctx =
            TraceContext::parse("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01").unwrap();

        assert_eq!(ctx.trace_id(), "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(ctx.parent_span_id(), Some("b7ad6b7169203331"));
        assert!(ctx.is_sampled());
    }

    #[test]
    fn test_parse_invalid_traceparent() {
        // Wrong version
        assert!(
            TraceContext::parse("01-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01")
                .is_none()
        );

        // All zeros trace-id
        assert!(
            TraceContext::parse("00-00000000000000000000000000000000-b7ad6b7169203331-01")
                .is_none()
        );

        // All zeros parent-id
        assert!(
            TraceContext::parse("00-0af7651916cd43dd8448eb211c80319c-0000000000000000-01")
                .is_none()
        );

        // Wrong length
        assert!(TraceContext::parse("00-0af7651916cd43dd-b7ad6b7169203331-01").is_none());

        // Invalid hex
        assert!(
            TraceContext::parse("00-zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz-b7ad6b7169203331-01")
                .is_none()
        );
    }

    #[test]
    fn test_generate_new_context() {
        let ctx = TraceContext::new();
        assert_eq!(ctx.trace_id().len(), 32);
        assert_eq!(ctx.span_id().len(), 16);
        assert!(ctx.parent_span_id().is_none());
        assert!(ctx.is_sampled());
    }

    #[test]
    fn test_traceparent_format() {
        let ctx =
            TraceContext::parse("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01").unwrap();
        let header = ctx.traceparent();

        // trace_id stays the same, span_id is new
        assert!(header.starts_with("00-0af7651916cd43dd8448eb211c80319c-"));
        assert!(header.ends_with("-01"));
        assert_eq!(header.len(), 55);
    }

    #[test]
    fn test_short_id_format() {
        let ctx = TraceContext::new();
        let short = ctx.short_id();

        assert_eq!(short.len(), 17);
        assert_eq!(&short[12..13], "-");

        // First 12 chars should match trace_id prefix
        assert_eq!(&short[..12], &ctx.trace_id()[..12]);
        // Last 4 chars should match span_id prefix
        assert_eq!(&short[13..17], &ctx.span_id()[..4]);
    }

    #[test]
    fn test_unique_span_ids() {
        let ctx1 = TraceContext::new();
        let ctx2 = TraceContext::new();
        let ctx3 = TraceContext::new();

        assert_ne!(ctx1.span_id(), ctx2.span_id());
        assert_ne!(ctx2.span_id(), ctx3.span_id());
        assert_ne!(ctx1.span_id(), ctx3.span_id());
    }

    #[test]
    fn test_copy_semantics() {
        let ctx1 = TraceContext::new();
        let ctx2 = ctx1; // Copy, not move

        assert_eq!(ctx1.trace_id(), ctx2.trace_id());
        assert_eq!(ctx1.span_id(), ctx2.span_id());
    }

    #[test]
    fn test_size() {
        // Verify the struct is reasonably sized for stack allocation
        assert!(std::mem::size_of::<TraceContext>() <= 128);
    }
}
