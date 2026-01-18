<?php
/**
 * Test: headers should arrive BEFORE body using SSE.
 *
 * Run: curl -N http://localhost:8080/test_headers_delay.php
 *
 * Expected: headers appear immediately, events appear with 2s delay between them.
 */

// SSE headers - no Content-Length, streaming enabled
header('Content-Type: text/event-stream');
header('Cache-Control: no-cache');
header('X-Accel-Buffering: no');
header('X-Timestamp-Headers: ' . microtime(true));

// Disable buffering
if (ob_get_level()) ob_end_clean();
ob_implicit_flush(true);

// Event 1 - should arrive immediately with headers
echo "data: Event 1 at " . microtime(true) . "\n\n";
flush();

// Wait 2 seconds
sleep(2);

// Event 2 - should arrive 2 seconds later
echo "data: Event 2 at " . microtime(true) . "\n\n";
flush();

// Wait 2 seconds
sleep(2);

// Event 3 - should arrive 2 seconds after event 2
echo "data: Event 3 at " . microtime(true) . "\n\n";
