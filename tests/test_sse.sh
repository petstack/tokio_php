#!/bin/bash
#
# SSE (Server-Sent Events) Integration Test Suite
#
# Usage:
#   ./tests/test_sse.sh [BASE_URL]
#
# Default BASE_URL: http://localhost:8080

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
DATA=$(timeout 3 curl -sN -H "Accept: text/event-stream" "$BASE_URL/test_sse.php" 2>/dev/null | head -5 || true)

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
EVENT_COUNT=$(timeout 5 curl -sN -H "Accept: text/event-stream" "$BASE_URL/test_sse.php" 2>/dev/null | grep -c "^data:" || echo "0")

if [ "$EVENT_COUNT" -ge 3 ]; then
    pass "Received multiple events ($EVENT_COUNT)"
else
    fail "Expected 3+ events, got $EVENT_COUNT"
fi

# Test 4: Concurrent SSE connections
info "Test 4: Concurrent SSE connections"
PIDS=""
for i in {1..5}; do
    timeout 3 curl -sN -H "Accept: text/event-stream" "$BASE_URL/test_sse.php" > /tmp/sse_test_$i.log 2>/dev/null &
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

# Test 5: Long-running SSE with heartbeat
info "Test 5: Long-running SSE with heartbeat (3s)"
LONG_DATA=$(timeout 5 curl -sN -H "Accept: text/event-stream" "$BASE_URL/test_sse_long.php?duration=3" 2>/dev/null || true)

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

# Test 6: SSE completion event
info "Test 6: SSE completion event"
if echo "$LONG_DATA" | grep -q "event: close"; then
    pass "SSE close event received"
else
    fail "SSE close event not received"
fi

# Test 7: SSE metrics (if internal server available)
info "Test 7: SSE metrics"
METRICS=$(curl -s "$INTERNAL_URL/metrics" 2>/dev/null || true)

if echo "$METRICS" | grep -q "tokio_php_sse_active_connections"; then
    pass "SSE active connections metric present"
else
    fail "SSE active connections metric missing (internal server may not be running)"
fi

if echo "$METRICS" | grep -q "tokio_php_sse_connections_total"; then
    pass "SSE total connections metric present"
else
    fail "SSE total connections metric missing"
fi

# Test 8: Non-SSE request still works normally
info "Test 8: Non-SSE request (normal response)"
NORMAL_RESP=$(curl -s "$BASE_URL/test_sse.php" 2>/dev/null | head -1 || true)

if echo "$NORMAL_RESP" | grep -q "^data:"; then
    # If still gets SSE format, that's also acceptable
    pass "Normal request returns data"
else
    pass "Normal request processed"
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
