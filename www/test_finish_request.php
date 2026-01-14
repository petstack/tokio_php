<?php
/**
 * Test tokio_finish_request() - analog of fastcgi_finish_request()
 *
 * This demonstrates sending response to client before script completes.
 */

header('Content-Type: text/plain');
header('X-Test-Header: before-finish');

echo "Response sent to client!\n";
echo "Time: " . date('H:i:s') . "\n";

// Send response NOW - client gets it immediately
$result = tokio_finish_request();

// Everything after this runs in background (client doesn't wait)
// Add a header AFTER finish - should NOT be sent to client
header('X-After-Finish: should-not-appear');

// This output should NOT be sent to client
echo "\n--- This should NOT appear in response ---\n";
echo "Background processing started...\n";

// Simulate slow background work
// (In real app: send emails, log to database, cleanup, etc.)
usleep(100000); // 100ms

echo "Background work done!\n";

// File to prove background execution happened
file_put_contents('/tmp/finish_request_test.txt',
    "Finish request test completed at " . date('Y-m-d H:i:s') . "\n" .
    "tokio_finish_request() returned: " . ($result ? 'true' : 'false') . "\n"
);
