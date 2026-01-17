<?php
/**
 * Test header capturing
 */

header("X-Custom-Header: test-value");
header("X-Another-Header: another-value");

echo "Headers set. Check response headers.\n";
echo "Apache headers_list: " . json_encode(headers_list()) . "\n";
