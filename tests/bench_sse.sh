#!/bin/bash
#
# SSE (Server-Sent Events) Benchmark Script
#
# Measures:
# - Connection establishment rate
# - Concurrent connection handling
# - Memory usage under load
#
# Usage:
#   ./tests/bench_sse.sh [BASE_URL] [INTERNAL_URL]
#
# Default BASE_URL: http://localhost:8080
# Default INTERNAL_URL: http://localhost:9090

set -e

BASE_URL="${1:-http://localhost:8080}"
INTERNAL_URL="${2:-http://localhost:9090}"

echo "=== SSE Benchmark Suite ==="
echo "Base URL: $BASE_URL"
echo "Internal URL: $INTERNAL_URL"
echo ""

# Check if server is running
if ! curl -s "$BASE_URL/" > /dev/null 2>&1; then
    echo "Error: Server not responding at $BASE_URL"
    exit 1
fi

# Get initial metrics
get_metrics() {
    curl -s "$INTERNAL_URL/metrics" 2>/dev/null || echo ""
}

get_sse_total() {
    get_metrics | grep "tokio_php_sse_connections_total" | awk '{print $2}' | tr -d '\r\n' || echo "0"
}

get_sse_active() {
    get_metrics | grep "tokio_php_sse_active_connections" | awk '{print $2}' | tr -d '\r\n' || echo "0"
}

get_sse_chunks() {
    get_metrics | grep "tokio_php_sse_chunks_total" | awk '{print $2}' | tr -d '\r\n' || echo "0"
}

get_sse_bytes() {
    get_metrics | grep "tokio_php_sse_bytes_total" | awk '{print $2}' | tr -d '\r\n' || echo "0"
}

# Benchmark 1: Short SSE connections (connection rate)
echo "Benchmark 1: Short SSE connections (100 connections)"
echo "─────────────────────────────────────────────────────"

INITIAL_TOTAL=$(get_sse_total)
START_TIME=$(date +%s.%N)

for i in {1..100}; do
    timeout 2 curl -sN -H "Accept: text/event-stream" "$BASE_URL/test_sse.php" > /dev/null 2>&1 &
done

# Wait for all to complete
wait 2>/dev/null || true

END_TIME=$(date +%s.%N)
FINAL_TOTAL=$(get_sse_total)

DURATION=$(echo "$END_TIME - $START_TIME" | bc)
CONNECTIONS=$((FINAL_TOTAL - INITIAL_TOTAL))

echo "Connections: $CONNECTIONS"
printf "Duration: %.2f seconds\n" "$DURATION"
printf "Rate: %.1f connections/second\n" "$(echo "$CONNECTIONS / $DURATION" | bc -l)"
echo ""

# Benchmark 2: Concurrent SSE connections
echo "Benchmark 2: Concurrent SSE connections (50 simultaneous)"
echo "──────────────────────────────────────────────────────────"

# Start 50 concurrent connections
PIDS=""
for i in {1..50}; do
    timeout 5 curl -sN -H "Accept: text/event-stream" "$BASE_URL/test_sse_long.php?duration=3" > /dev/null 2>&1 &
    PIDS="$PIDS $!"
done

# Give connections time to establish
sleep 1

ACTIVE=$(get_sse_active)
echo "Active connections: $ACTIVE"

# Wait for completion
wait $PIDS 2>/dev/null || true

ACTIVE_AFTER=$(get_sse_active)
echo "Active after completion: $ACTIVE_AFTER"
echo ""

# Benchmark 3: Chunk throughput
echo "Benchmark 3: Chunk throughput"
echo "─────────────────────────────"

INITIAL_CHUNKS=$(get_sse_chunks)
INITIAL_BYTES=$(get_sse_bytes)

# Run 10 connections for 3 seconds each (producing ~3 events each)
for i in {1..10}; do
    timeout 4 curl -sN -H "Accept: text/event-stream" "$BASE_URL/test_sse_long.php?duration=3" > /dev/null 2>&1 &
done
wait 2>/dev/null || true

FINAL_CHUNKS=$(get_sse_chunks)
FINAL_BYTES=$(get_sse_bytes)

CHUNKS=$((FINAL_CHUNKS - INITIAL_CHUNKS))
BYTES=$((FINAL_BYTES - INITIAL_BYTES))

echo "Chunks sent: $CHUNKS"
echo "Bytes sent: $BYTES"
printf "Average bytes/chunk: %.1f\n" "$(echo "$BYTES / $CHUNKS" | bc -l 2>/dev/null || echo "0")"
echo ""

# Benchmark 4: Memory usage under SSE load
echo "Benchmark 4: Memory usage during SSE load"
echo "──────────────────────────────────────────"

# Get initial memory
HEALTH_BEFORE=$(curl -s "$INTERNAL_URL/health" 2>/dev/null)
echo "Health before: $HEALTH_BEFORE"

# Create sustained load
for i in {1..30}; do
    timeout 5 curl -sN -H "Accept: text/event-stream" "$BASE_URL/test_sse_long.php?duration=4" > /dev/null 2>&1 &
done

sleep 2

HEALTH_DURING=$(curl -s "$INTERNAL_URL/health" 2>/dev/null)
ACTIVE_DURING=$(get_sse_active)
echo "Active during load: $ACTIVE_DURING"
echo "Health during: $HEALTH_DURING"

wait 2>/dev/null || true

HEALTH_AFTER=$(curl -s "$INTERNAL_URL/health" 2>/dev/null)
echo "Health after: $HEALTH_AFTER"
echo ""

# Summary
echo "=== Benchmark Summary ==="
TOTAL=$(get_sse_total)
TOTAL_CHUNKS=$(get_sse_chunks)
TOTAL_BYTES=$(get_sse_bytes)

echo "Total SSE connections: $TOTAL"
echo "Total chunks sent: $TOTAL_CHUNKS"
echo "Total bytes sent: $TOTAL_BYTES"

# Final metrics dump
echo ""
echo "=== Full SSE Metrics ==="
get_metrics | grep -E "^tokio_php_sse" || echo "(no SSE metrics found)"
