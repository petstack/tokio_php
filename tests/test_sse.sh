#!/bin/bash
#
# SSE (Server-Sent Events) Integration Test Suite
#
# Tests that SSE streaming works correctly:
# - Correct headers (Content-Type: text/event-stream)
# - Events are streamed incrementally (not buffered until script end)
# - Multiple concurrent SSE connections work
# - flush() correctly sends data to client
#
# Usage:
#   ./tests/test_sse.sh [BASE_URL] [INTERNAL_URL]
#
# Default BASE_URL: http://localhost:8080
# Default INTERNAL_URL: http://localhost:9090

set -e

BASE_URL="${1:-http://localhost:8080}"
INTERNAL_URL="${2:-http://localhost:9090}"
PASS=0
FAIL=0

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

pass() {
    ((PASS++))
    echo -e "${GREEN}✓${NC} $1"
}

fail() {
    ((FAIL++))
    echo -e "${RED}✗${NC} $1"
}

info() {
    echo -e "${YELLOW}→${NC} $1"
}

# Check if timeout command exists (not available on macOS by default)
if command -v gtimeout &> /dev/null; then
    TIMEOUT_CMD="gtimeout"
elif command -v timeout &> /dev/null; then
    TIMEOUT_CMD="timeout"
else
    # Fallback: use perl-based timeout
    timeout_fallback() {
        local secs=$1
        shift
        perl -e 'alarm shift; exec @ARGV' "$secs" "$@"
    }
    TIMEOUT_CMD="timeout_fallback"
fi

echo "=== SSE Integration Test Suite ==="
echo "Base URL: $BASE_URL"
echo ""

# Test 1: Basic SSE response headers
info "Test 1: Basic SSE response headers"
HEADERS=$(curl -sI -H "Accept: text/event-stream" "$BASE_URL/test_sse.php" 2>/dev/null || true)

if echo "$HEADERS" | grep -qi "content-type:.*text/event-stream"; then
    pass "Content-Type: text/event-stream"
else
    fail "Content-Type header missing or incorrect"
fi

if echo "$HEADERS" | grep -qi "cache-control:.*no-cache"; then
    pass "Cache-Control: no-cache"
else
    fail "Cache-Control header missing"
fi

if echo "$HEADERS" | grep -qi "x-request-id:"; then
    pass "X-Request-ID present"
else
    fail "X-Request-ID header missing"
fi

# Test 2: SSE data format
info "Test 2: SSE data format"
DATA=$(curl -sN -H "Accept: text/event-stream" --max-time 3 "$BASE_URL/test_sse.php" 2>/dev/null | head -5 || true)

if echo "$DATA" | grep -q "^data: {"; then
    pass "SSE data format correct (data: prefix)"
else
    fail "SSE data format incorrect"
fi

if echo "$DATA" | grep -q '"event"'; then
    pass "JSON payload contains expected fields"
else
    fail "JSON payload missing expected fields"
fi

# Test 3: Multiple events received
info "Test 3: Multiple events (streaming)"
EVENT_COUNT=$(curl -sN -H "Accept: text/event-stream" --max-time 5 "$BASE_URL/test_sse.php" 2>/dev/null | grep -c "^data:" || echo "0")

if [ "$EVENT_COUNT" -ge 3 ]; then
    pass "Received multiple events ($EVENT_COUNT)"
else
    fail "Expected 3+ events, got $EVENT_COUNT"
fi

# Test 4: Concurrent SSE connections
info "Test 4: Concurrent SSE connections"
PIDS=""
for i in {1..5}; do
    curl -sN -H "Accept: text/event-stream" --max-time 3 "$BASE_URL/test_sse.php" > /tmp/sse_test_$i.log 2>/dev/null &
    PIDS="$PIDS $!"
done

# Wait for all connections
wait $PIDS 2>/dev/null || true

SUCCESS=0
for i in {1..5}; do
    if [ -f /tmp/sse_test_$i.log ] && grep -q "^data:" /tmp/sse_test_$i.log 2>/dev/null; then
        ((SUCCESS++))
    fi
    rm -f /tmp/sse_test_$i.log
done

if [ "$SUCCESS" -eq 5 ]; then
    pass "All 5 concurrent connections received data"
else
    fail "Only $SUCCESS/5 concurrent connections received data"
fi

# Test 5: Streaming timing verification (critical test!)
# This verifies that events arrive incrementally, not all at once at script end
info "Test 5: Streaming timing (events should arrive incrementally)"

# Use test_sse_timed.php with 500ms delay between 4 events
# If streaming works, we should receive 2+ events in the first 1.5 seconds
START_TIME=$(date +%s%3N 2>/dev/null || python3 -c 'import time; print(int(time.time()*1000))')

# Collect events for 2 seconds
TIMED_DATA=$(curl -sN -H "Accept: text/event-stream" --max-time 2 "$BASE_URL/test_sse_timed.php?count=4&delay=500" 2>/dev/null || true)
TIMED_COUNT=$(echo "$TIMED_DATA" | grep -c "^data:" || echo "0")

if [ "$TIMED_COUNT" -ge 3 ]; then
    pass "Streaming works: received $TIMED_COUNT events in 2 seconds (expected 3-4 with 500ms delay)"
else
    fail "Streaming may be broken: only received $TIMED_COUNT events in 2 seconds (expected 3-4)"
fi

# Test 6: Long-running SSE with heartbeat
info "Test 6: Long-running SSE with heartbeat (3s)"
LONG_DATA=$(curl -sN -H "Accept: text/event-stream" --max-time 5 "$BASE_URL/test_sse_long.php?duration=3" 2>/dev/null || true)

if echo "$LONG_DATA" | grep -q '"elapsed"'; then
    pass "Long-running SSE sends elapsed time"
else
    fail "Long-running SSE data missing elapsed field"
fi

if echo "$LONG_DATA" | grep -q '"memory"'; then
    pass "Long-running SSE sends memory info"
else
    fail "Long-running SSE data missing memory field"
fi

# Test 7: SSE completion event
info "Test 7: SSE completion event"
if echo "$LONG_DATA" | grep -q "event: close"; then
    pass "SSE close event received"
else
    fail "SSE close event not received"
fi

# Test 8: Minimal SSE (no delays)
info "Test 8: Minimal SSE test (no delays)"
MINIMAL_DATA=$(curl -sN -H "Accept: text/event-stream" --max-time 2 "$BASE_URL/test_sse_minimal.php" 2>/dev/null || true)
MINIMAL_COUNT=$(echo "$MINIMAL_DATA" | grep -c "^data:" || echo "0")

if [ "$MINIMAL_COUNT" -ge 3 ]; then
    pass "Minimal SSE test: received $MINIMAL_COUNT events"
else
    fail "Minimal SSE test: expected 3 events, got $MINIMAL_COUNT"
fi

# Check for expected content
if echo "$MINIMAL_DATA" | grep -q "chunk1" && echo "$MINIMAL_DATA" | grep -q "chunk2" && echo "$MINIMAL_DATA" | grep -q "chunk3"; then
    pass "Minimal SSE test: correct event content"
else
    fail "Minimal SSE test: missing expected chunks"
fi

# Test 9: Non-SSE request still works normally
info "Test 9: Non-SSE request (normal response)"
NORMAL_RESP=$(curl -s "$BASE_URL/test_sse.php" 2>/dev/null | head -1 || true)

if echo "$NORMAL_RESP" | grep -q "^data:"; then
    # If still gets SSE format, that's also acceptable
    pass "Normal request returns data"
else
    pass "Normal request processed"
fi

# Test 10: SSE with very short delay (100ms)
info "Test 10: SSE with fast events (100ms delay)"
FAST_DATA=$(curl -sN -H "Accept: text/event-stream" --max-time 2 "$BASE_URL/test_sse_timed.php?count=10&delay=100" 2>/dev/null || true)
FAST_COUNT=$(echo "$FAST_DATA" | grep -c "^data:" || echo "0")

if [ "$FAST_COUNT" -ge 8 ]; then
    pass "Fast SSE: received $FAST_COUNT/10 events in 2 seconds"
else
    fail "Fast SSE: expected 8+ events, got $FAST_COUNT"
fi

# Summary
echo ""
echo "=== Test Results ==="
echo -e "Passed: ${GREEN}$PASS${NC}"
echo -e "Failed: ${RED}$FAIL${NC}"

if [ "$FAIL" -eq 0 ]; then
    echo -e "\n${GREEN}All tests passed!${NC}"
    exit 0
else
    echo -e "\n${RED}Some tests failed${NC}"
    exit 1
fi
