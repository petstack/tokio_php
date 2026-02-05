<?php
// Test if OPcache works differently across threads
$status = opcache_get_status(false);
$worker_id = $_SERVER['TOKIO_WORKER_ID'] ?? 'unknown';

echo "Worker: $worker_id\n";
echo "PID: " . getmypid() . "\n";
echo "Thread ID: " . (function_exists('zend_thread_id') ? zend_thread_id() : 'N/A') . "\n";
echo "opcache_enabled: " . ($status['opcache_enabled'] ? 'YES' : 'NO') . "\n";
echo "num_cached_scripts: " . $status['opcache_statistics']['num_cached_scripts'] . "\n";
echo "hits: " . $status['opcache_statistics']['hits'] . "\n";
echo "misses: " . $status['opcache_statistics']['misses'] . "\n";

// Try to compile this file explicitly
$result = opcache_compile_file(__FILE__);
echo "compile_file result: " . ($result ? 'true' : 'false') . "\n";

// Check again
$status2 = opcache_get_status(false);
echo "After compile - cached: " . $status2['opcache_statistics']['num_cached_scripts'] . "\n";
