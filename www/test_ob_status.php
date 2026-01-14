<?php
// Check output buffering status

echo "Output buffer level: " . ob_get_level() . "\n";
echo "Headers sent: " . (headers_sent() ? 'yes' : 'no') . "\n";
echo "Buffer contents length: " . strlen(ob_get_contents() ?: '') . "\n";
