<?php
/**
 * Test tokio_finish_request() streaming early response
 */

// Output that will be sent to client
echo "Response sent to client at: " . date('H:i:s') . "\n";
header("X-Test-Header: before-finish");
header("X-Response-Time: " . microtime(true));

// Signal to send response immediately
$result = tokio_finish_request();
echo "This line should NOT be in response (finish_request returned: " . ($result ? "true" : "false") . ")\n";

// Simulate background work (client should not wait for this)
usleep(100000); // 100ms

// This output should be discarded
echo "Background work completed at: " . date('H:i:s') . "\n";
