<?php
/**
 * Debug test for tokio_finish_request()
 */

header('Content-Type: text/plain');

echo "Before finish: 123456789\n";  // 30 bytes including newline

// Call finish request and capture details
$result = tokio_finish_request();

// This should NOT appear
echo "AFTER: this should be truncated\n";
