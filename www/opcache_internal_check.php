<?php
// Check internal OPcache state
$status = opcache_get_status(false);

echo "=== OPcache Internal Check ===\n\n";

echo "opcache_enabled (from status): " . ($status['opcache_enabled'] ? 'YES' : 'NO') . "\n";
echo "cache_full: " . ($status['cache_full'] ? 'YES' : 'NO') . "\n";
echo "restart_pending: " . ($status['restart_pending'] ? 'YES' : 'NO') . "\n";
echo "restart_in_progress: " . ($status['restart_in_progress'] ? 'YES' : 'NO') . "\n";

echo "\n=== Testing script caching flow ===\n";

// Create a temp file to test caching
$temp_file = '/tmp/opcache_test_' . getmypid() . '.php';
file_put_contents($temp_file, '<?php return 42;');

echo "Created temp file: $temp_file\n";

// Check if it's cached before
echo "Before include - is_cached: " . (opcache_is_script_cached($temp_file) ? 'YES' : 'NO') . "\n";

// Include it (should trigger caching)
$result = include($temp_file);
echo "Include result: $result\n";

// Check status after include
$status2 = opcache_get_status(false);
echo "After include - num_cached_scripts: " . $status2['opcache_statistics']['num_cached_scripts'] . "\n";
echo "After include - is_cached: " . (opcache_is_script_cached($temp_file) ? 'YES' : 'NO') . "\n";

// Try compile_file
$compile_result = opcache_compile_file($temp_file);
echo "compile_file result: " . ($compile_result ? 'true' : 'false') . "\n";

// Check status after compile
$status3 = opcache_get_status(false);
echo "After compile - num_cached_scripts: " . $status3['opcache_statistics']['num_cached_scripts'] . "\n";
echo "After compile - is_cached: " . (opcache_is_script_cached($temp_file) ? 'YES' : 'NO') . "\n";

// Clean up
unlink($temp_file);

echo "\n=== Checking if OPcache hooks are installed ===\n";
// This is a bit of a hack - if OPcache is properly hooked, compile errors should be caught
echo "zend_compile_file hook test - if we got here, basic compilation works\n";

echo "\n=== Statistics ===\n";
echo "hits: " . $status3['opcache_statistics']['hits'] . "\n";
echo "misses: " . $status3['opcache_statistics']['misses'] . "\n";
echo "oom_restarts: " . $status3['opcache_statistics']['oom_restarts'] . "\n";
echo "hash_restarts: " . $status3['opcache_statistics']['hash_restarts'] . "\n";
