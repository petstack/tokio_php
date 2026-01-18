#!/bin/bash
# Run integration tests against a running tokio_php server
#
# Usage:
#   ./scripts/run-integration-tests.sh
#
# Prerequisites:
#   - Docker container running: docker compose up -d
#   - Server healthy: curl http://localhost:9090/health

set -e

# Configuration
SERVER_URL="${TEST_SERVER_URL:-http://localhost:8080}"
INTERNAL_URL="${TEST_INTERNAL_URL:-http://localhost:9090}"
CURL_TIMEOUT="${CURL_TIMEOUT:-10}"
RETRY_COUNT="${RETRY_COUNT:-3}"

# Retry wrapper for curl
curl_retry() {
    local url="$1"
    shift
    local attempt=1
    while [ $attempt -le $RETRY_COUNT ]; do
        result=$(curl --max-time "$CURL_TIMEOUT" "$@" "$url" 2>/dev/null) && echo "$result" && return 0
        echo "  (retry $attempt/$RETRY_COUNT for $url)" >&2
        sleep 1
        attempt=$((attempt + 1))
    done
    return 1
}

echo "=================================="
echo "  tokio_php Integration Tests"
echo "=================================="
echo ""
echo "Server URL: $SERVER_URL"
echo "Internal URL: $INTERNAL_URL"
echo ""

PASSED=0
FAILED=0

pass() {
    echo "  [PASS] $1"
    PASSED=$((PASSED + 1))
}

fail() {
    echo "  [FAIL] $1"
    echo "         Expected: $2"
    echo "         Got: $3"
    FAILED=$((FAILED + 1))
}

# Check if server is running
echo "Checking server health..."
health=$(curl -sf --max-time "$CURL_TIMEOUT" "$INTERNAL_URL/health" 2>/dev/null || echo "")
if [ -z "$health" ]; then
    echo "ERROR: Server is not running. Start it with: docker compose up -d"
    exit 1
fi
echo "Server is healthy"

# Warm up the server with a few requests
echo "Warming up server..."
for i in {1..3}; do
    curl -sf --max-time "$CURL_TIMEOUT" "$SERVER_URL/index.php" > /dev/null 2>&1 || true
done
echo ""

# HTTP Status Tests
echo "=== HTTP Status Tests ==="

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time "$CURL_TIMEOUT" "$SERVER_URL/index.php")
[ "$status" = "200" ] && pass "GET /index.php returns 200" || fail "GET /index.php" "200" "$status"

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time "$CURL_TIMEOUT" "$SERVER_URL/bench.php")
[ "$status" = "200" ] && pass "GET /bench.php returns 200" || fail "GET /bench.php" "200" "$status"

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time "$CURL_TIMEOUT" "$SERVER_URL/nonexistent.php")
[ "$status" = "404" ] && pass "GET /nonexistent.php returns 404" || fail "GET /nonexistent.php" "404" "$status"

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time "$CURL_TIMEOUT" "$SERVER_URL/hello.php?name=Test")
[ "$status" = "200" ] && pass "GET /hello.php?name=Test returns 200" || fail "GET /hello.php?name=Test" "200" "$status"

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time "$CURL_TIMEOUT" "$SERVER_URL/styles.css")
[ "$status" = "200" ] && pass "GET /styles.css returns 200" || fail "GET /styles.css" "200" "$status"

echo ""
echo "=== Content Tests ==="

body=$(curl_retry "$SERVER_URL/bench.php" -s)
[ "$body" = "ok" ] && pass "bench.php returns 'ok'" || fail "bench.php body" "ok" "$body"

body=$(curl_retry "$SERVER_URL/hello.php?name=TestUser" -s)
if echo "$body" | grep -q "Hello, TestUser!"; then
    pass "hello.php shows 'Hello, TestUser!'"
else
    fail "hello.php content" "contains 'Hello, TestUser!'" "${body:0:50}..."
fi

body=$(curl_retry "$SERVER_URL/index.php" -s)
if echo "$body" | grep -q "tokio_php"; then
    pass "index.php contains 'tokio_php'"
else
    fail "index.php content" "contains 'tokio_php'" "${body:0:50}..."
fi

echo ""
echo "=== Header Tests ==="

headers=$(curl -sI --max-time "$CURL_TIMEOUT" "$SERVER_URL/bench.php" 2>/dev/null || echo "")
if echo "$headers" | grep -qi "x-request-id"; then
    pass "X-Request-ID header present"
else
    fail "X-Request-ID header" "present" "missing"
fi

headers=$(curl -sI --max-time "$CURL_TIMEOUT" "$SERVER_URL/styles.css" 2>/dev/null || echo "")
if echo "$headers" | grep -qi "cache-control"; then
    pass "Cache-Control header on static files"
else
    fail "Cache-Control header" "present" "missing"
fi

if echo "$headers" | grep -qi "etag"; then
    pass "ETag header on static files"
else
    fail "ETag header" "present" "missing"
fi

if echo "$headers" | grep -qi "content-type: text/css"; then
    pass "Content-Type: text/css on CSS files"
else
    fail "Content-Type for CSS" "text/css" "$(echo "$headers" | grep -i content-type)"
fi

echo ""
echo "=== POST Tests ==="

body=$(curl -s --max-time "$CURL_TIMEOUT" -X POST -d "name=John&email=john@test.com" "$SERVER_URL/form.php" 2>/dev/null || echo "")
if echo "$body" | grep -q "John"; then
    pass "POST form data processed"
else
    fail "POST form" "contains 'John'" "${body:0:50}..."
fi

echo ""
echo "=== Compression Tests ==="

headers=$(curl -sI --max-time "$CURL_TIMEOUT" -H "Accept-Encoding: br" "$SERVER_URL/index.php" 2>/dev/null || echo "")
if echo "$headers" | grep -qi "content-encoding: br"; then
    pass "Brotli compression applied"
else
    echo "  [SKIP] Brotli compression (may be disabled or response too small)"
fi

headers=$(curl -sI --max-time "$CURL_TIMEOUT" -H "Accept-Encoding: br" "$SERVER_URL/bench.php" 2>/dev/null || echo "")
if ! echo "$headers" | grep -qi "content-encoding: br"; then
    pass "Small responses not compressed"
else
    fail "Small response compression" "not compressed" "compressed"
fi

echo ""
echo "=== Internal Server Tests ==="

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time "$CURL_TIMEOUT" "$INTERNAL_URL/health" 2>/dev/null || echo "000")
[ "$status" = "200" ] && pass "GET /health returns 200" || fail "GET /health" "200" "$status"

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time "$CURL_TIMEOUT" "$INTERNAL_URL/metrics" 2>/dev/null || echo "000")
[ "$status" = "200" ] && pass "GET /metrics returns 200" || fail "GET /metrics" "200" "$status"

body=$(curl -s --max-time "$CURL_TIMEOUT" "$INTERNAL_URL/health" 2>/dev/null || echo "")
if echo "$body" | grep -q '"status":"ok"'; then
    pass "/health returns JSON with status"
else
    fail "/health JSON" "contains '\"status\":\"ok\"'" "${body:0:50}..."
fi

body=$(curl -s --max-time "$CURL_TIMEOUT" "$INTERNAL_URL/metrics" 2>/dev/null || echo "")
if echo "$body" | grep -q "tokio_php_uptime_seconds"; then
    pass "/metrics contains uptime"
else
    fail "/metrics" "contains 'tokio_php_uptime_seconds'" "missing"
fi

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time "$CURL_TIMEOUT" "$INTERNAL_URL/unknown" 2>/dev/null || echo "000")
[ "$status" = "404" ] && pass "GET /unknown returns 404" || fail "GET /unknown" "404" "$status"

echo ""
echo "=== Security Tests ==="

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time "$CURL_TIMEOUT" "$SERVER_URL/../../../etc/passwd" 2>/dev/null || echo "000")
if [ "$status" = "404" ] || [ "$status" = "400" ] || [ "$status" = "403" ]; then
    pass "Path traversal blocked"
else
    fail "Path traversal protection" "4xx" "$status"
fi

echo ""
echo "=== Request Time Tests ==="

# Test REQUEST_TIME and REQUEST_TIME_FLOAT
body=$(curl -s --max-time "$CURL_TIMEOUT" "$SERVER_URL/test_request_time.php" 2>/dev/null || echo "")
if echo "$body" | grep -q '"is_valid": *true'; then
    pass "REQUEST_TIME_FLOAT is valid"
else
    fail "REQUEST_TIME_FLOAT" "is_valid: true" "${body:0:100}..."
fi

# Check REQUEST_TIME is positive (handles JSON with spaces)
request_time=$(echo "$body" | tr -d ' \n' | grep -o '"request_time":[0-9]*' | cut -d':' -f2)
if [ -n "$request_time" ] && [ "$request_time" -gt 0 ] 2>/dev/null; then
    pass "REQUEST_TIME is positive ($request_time)"
else
    fail "REQUEST_TIME" "> 0" "$request_time"
fi

# Check delay_ms is reasonable (< 1000ms typically)
delay_ms=$(echo "$body" | tr -d ' \n' | grep -o '"delay_ms":[0-9.]*' | cut -d':' -f2 | cut -d'.' -f1)
if [ -n "$delay_ms" ] && [ "$delay_ms" -lt 5000 ] 2>/dev/null; then
    pass "Request processing delay reasonable (${delay_ms}ms)"
else
    fail "Request delay" "< 5000ms" "${delay_ms}ms"
fi

echo ""
echo "=== SSE (Server-Sent Events) Tests ==="

# Test SSE headers
headers=$(curl -sI --max-time "$CURL_TIMEOUT" -H "Accept: text/event-stream" "$SERVER_URL/test_sse_minimal.php" 2>/dev/null || echo "")
if echo "$headers" | grep -qi "content-type:.*text/event-stream"; then
    pass "SSE Content-Type header"
else
    fail "SSE Content-Type" "text/event-stream" "$(echo "$headers" | grep -i content-type)"
fi

# Test minimal SSE events
body=$(curl -sN --max-time 3 -H "Accept: text/event-stream" "$SERVER_URL/test_sse_minimal.php" 2>/dev/null || echo "")
event_count=$(echo "$body" | grep -c "^data:" || echo "0")
if [ "$event_count" -ge 3 ]; then
    pass "Minimal SSE: received $event_count events"
else
    fail "Minimal SSE events" ">= 3" "$event_count"
fi

# Test SSE streaming with delays (critical test for flush fix)
body=$(curl -sN --max-time 2 -H "Accept: text/event-stream" "$SERVER_URL/test_sse_timed.php?count=4&delay=500" 2>/dev/null || echo "")
event_count=$(echo "$body" | grep -c "^data:" || echo "0")
if [ "$event_count" -ge 3 ]; then
    pass "SSE streaming: received $event_count events in 2s (delay=500ms)"
else
    fail "SSE streaming" ">= 3 events in 2s" "$event_count events"
fi

# Test fast SSE events
body=$(curl -sN --max-time 2 -H "Accept: text/event-stream" "$SERVER_URL/test_sse_timed.php?count=10&delay=100" 2>/dev/null || echo "")
event_count=$(echo "$body" | grep -c "^data:" || echo "0")
if [ "$event_count" -ge 8 ]; then
    pass "Fast SSE: received $event_count/10 events (delay=100ms)"
else
    fail "Fast SSE events" ">= 8" "$event_count"
fi

echo ""
echo "=================================="
echo "  Results"
echo "=================================="
echo "  Passed: $PASSED"
echo "  Failed: $FAILED"
echo ""

if [ $FAILED -gt 0 ]; then
    echo "Some tests failed!"
    exit 1
else
    echo "All tests passed!"
    exit 0
fi
