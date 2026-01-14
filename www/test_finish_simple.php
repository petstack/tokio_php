<?php
// Simple test - no headers before finish_request

echo "BEFORE\n";

$result = tokio_finish_request();
echo "tokio_finish_request returned: " . ($result ? 'true' : 'false') . "\n";

echo "AFTER - should NOT appear\n";
