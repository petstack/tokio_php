<?php
/**
 * Test: chunked streaming with any Content-Type.
 *
 * Run: curl -N http://localhost:8080/test_chunked_streaming.php
 *
 * Expected: each JSON line arrives immediately (2s delay between them),
 * NOT buffered until the end. No Content-Length header should be present.
 */

// Set headers (must be before tokio_send_headers)
header('Content-Type: application/json');
header('Cache-Control: no-cache');
header('X-Test: chunked-streaming');
header('X-Timestamp-Start: ' . microtime(true));

// Enable streaming mode: send headers now, use chunked encoding
// After this call, all output is sent immediately via flush()
tokio_send_headers();

// First output - sent immediately
echo json_encode(['event' => 1, 'time' => microtime(true)]) . "\n";
flush();

// Wait 2 seconds
sleep(2);

// Second output - sent 2 seconds later
echo json_encode(['event' => 2, 'time' => microtime(true)]) . "\n";
flush();

// Wait 2 seconds
sleep(2);

// Third output - sent 2 seconds after event 2
echo json_encode(['event' => 3, 'time' => microtime(true)]) . "\n";
