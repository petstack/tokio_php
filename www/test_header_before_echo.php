<?php
// Test: can we add header after some output?

echo "First output\n";
echo "OB level: " . ob_get_level() . "\n";
echo "Headers sent before header(): " . (headers_sent() ? 'yes' : 'no') . "\n";

// Try to add header
header("X-Test: after-echo");
echo "Headers sent after header(): " . (headers_sent() ? 'yes' : 'no') . "\n";
