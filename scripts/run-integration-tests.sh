#!/bin/bash
# Run integration tests against a running tokio_php server
#
# Usage:
#   ./scripts/run-integration-tests.sh
#
# Prerequisites:
#   - Docker container running: docker compose up -d
#   - Server healthy: curl http://localhost:9091/health

set -e

# Configuration
SERVER_URL="${TEST_SERVER_URL:-http://localhost:8081}"
INTERNAL_URL="${TEST_INTERNAL_URL:-http://localhost:9091}"

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
health=$(curl -sf --max-time 5 "$INTERNAL_URL/health" 2>/dev/null || echo "")
if [ -z "$health" ]; then
    echo "ERROR: Server is not running. Start it with: docker compose up -d"
    exit 1
fi
echo "Server is healthy"
echo ""

# HTTP Status Tests
echo "=== HTTP Status Tests ==="

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 "$SERVER_URL/index.php")
[ "$status" = "200" ] && pass "GET /index.php returns 200" || fail "GET /index.php" "200" "$status"

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 "$SERVER_URL/bench.php")
[ "$status" = "200" ] && pass "GET /bench.php returns 200" || fail "GET /bench.php" "200" "$status"

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 "$SERVER_URL/nonexistent.php")
[ "$status" = "404" ] && pass "GET /nonexistent.php returns 404" || fail "GET /nonexistent.php" "404" "$status"

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 "$SERVER_URL/hello.php?name=Test")
[ "$status" = "200" ] && pass "GET /hello.php?name=Test returns 200" || fail "GET /hello.php?name=Test" "200" "$status"

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 "$SERVER_URL/styles.css")
[ "$status" = "200" ] && pass "GET /styles.css returns 200" || fail "GET /styles.css" "200" "$status"

echo ""
echo "=== Content Tests ==="

body=$(curl -s --max-time 5 "$SERVER_URL/bench.php")
[ "$body" = "ok" ] && pass "bench.php returns 'ok'" || fail "bench.php body" "ok" "$body"

body=$(curl -s --max-time 5 "$SERVER_URL/hello.php?name=TestUser")
if echo "$body" | grep -q "Hello, TestUser!"; then
    pass "hello.php shows 'Hello, TestUser!'"
else
    fail "hello.php content" "contains 'Hello, TestUser!'" "${body:0:50}..."
fi

body=$(curl -s --max-time 5 "$SERVER_URL/index.php")
if echo "$body" | grep -q "tokio_php"; then
    pass "index.php contains 'tokio_php'"
else
    fail "index.php content" "contains 'tokio_php'" "${body:0:50}..."
fi

echo ""
echo "=== Header Tests ==="

headers=$(curl -sI --max-time 5 "$SERVER_URL/bench.php")
if echo "$headers" | grep -qi "x-request-id"; then
    pass "X-Request-ID header present"
else
    fail "X-Request-ID header" "present" "missing"
fi

headers=$(curl -sI --max-time 5 "$SERVER_URL/styles.css")
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

body=$(curl -s --max-time 5 -X POST -d "name=John&email=john@test.com" "$SERVER_URL/form.php")
if echo "$body" | grep -q "John"; then
    pass "POST form data processed"
else
    fail "POST form" "contains 'John'" "${body:0:50}..."
fi

echo ""
echo "=== Compression Tests ==="

headers=$(curl -sI --max-time 5 -H "Accept-Encoding: br" "$SERVER_URL/index.php")
if echo "$headers" | grep -qi "content-encoding: br"; then
    pass "Brotli compression applied"
else
    echo "  [SKIP] Brotli compression (may be disabled or response too small)"
fi

headers=$(curl -sI --max-time 5 -H "Accept-Encoding: br" "$SERVER_URL/bench.php")
if ! echo "$headers" | grep -qi "content-encoding: br"; then
    pass "Small responses not compressed"
else
    fail "Small response compression" "not compressed" "compressed"
fi

echo ""
echo "=== Internal Server Tests ==="

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 "$INTERNAL_URL/health")
[ "$status" = "200" ] && pass "GET /health returns 200" || fail "GET /health" "200" "$status"

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 "$INTERNAL_URL/metrics")
[ "$status" = "200" ] && pass "GET /metrics returns 200" || fail "GET /metrics" "200" "$status"

body=$(curl -s --max-time 5 "$INTERNAL_URL/health")
if echo "$body" | grep -q '"status":"ok"'; then
    pass "/health returns JSON with status"
else
    fail "/health JSON" "contains '\"status\":\"ok\"'" "${body:0:50}..."
fi

body=$(curl -s --max-time 5 "$INTERNAL_URL/metrics")
if echo "$body" | grep -q "tokio_php_uptime_seconds"; then
    pass "/metrics contains uptime"
else
    fail "/metrics" "contains 'tokio_php_uptime_seconds'" "missing"
fi

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 "$INTERNAL_URL/unknown")
[ "$status" = "404" ] && pass "GET /unknown returns 404" || fail "GET /unknown" "404" "$status"

echo ""
echo "=== Security Tests ==="

status=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 "$SERVER_URL/../../../etc/passwd")
if [ "$status" = "404" ] || [ "$status" = "400" ] || [ "$status" = "403" ]; then
    pass "Path traversal blocked"
else
    fail "Path traversal protection" "4xx" "$status"
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
