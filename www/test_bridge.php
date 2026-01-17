<?php
// Test: Bridge-based communication (finish_request, heartbeat)
// The bridge library provides shared TLS between Rust and PHP.

header('Content-Type: text/plain');

echo "=== Bridge Communication Test ===\n\n";

// Test 1: Check functions exist
echo "1. Function availability:\n";
echo "   - tokio_finish_request: " . (function_exists('tokio_finish_request') ? 'yes' : 'no') . "\n";
echo "   - tokio_request_heartbeat: " . (function_exists('tokio_request_heartbeat') ? 'yes' : 'no') . "\n";
echo "\n";

// Test 2: Request ID and worker ID
echo "2. Request context:\n";
echo "   - Request ID: " . tokio_request_id() . "\n";
echo "   - Worker ID: " . tokio_worker_id() . "\n";
echo "\n";

// Test 3: Server info (includes build version)
echo "3. Server info:\n";
$info = tokio_server_info();
foreach ($info as $key => $value) {
    echo "   - $key: $value\n";
}
echo "\n";

// Test 4: Heartbeat (extends timeout via bridge)
echo "4. Heartbeat test:\n";
$heartbeat_result = tokio_request_heartbeat(10);
echo "   - tokio_request_heartbeat(10): " . ($heartbeat_result ? 'true' : 'false') . "\n";
echo "   (Returns false if no timeout configured or exceeds limit)\n";
echo "\n";

echo "=== Test Complete ===\n";
