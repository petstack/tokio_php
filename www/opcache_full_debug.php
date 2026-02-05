<?php
error_reporting(E_ALL);
ini_set('display_errors', 1);

echo "=== Full OPcache Debug ===\n\n";

$status = opcache_get_status(true);
$config = opcache_get_configuration();

echo "=== Configuration ===\n";
foreach ($config['directives'] as $key => $value) {
    if (strpos($key, 'opcache') === 0) {
        echo "$key: " . var_export($value, true) . "\n";
    }
}

echo "\n=== Status ===\n";
echo "opcache_enabled: " . ($status['opcache_enabled'] ? 'YES' : 'NO') . "\n";
echo "cache_full: " . ($status['cache_full'] ? 'YES' : 'NO') . "\n";
echo "restart_pending: " . ($status['restart_pending'] ? 'YES' : 'NO') . "\n";
echo "restart_in_progress: " . ($status['restart_in_progress'] ? 'YES' : 'NO') . "\n";

echo "\n=== Memory Usage ===\n";
if (isset($status['memory_usage'])) {
    foreach ($status['memory_usage'] as $k => $v) {
        echo "$k: $v\n";
    }
}

echo "\n=== Interned Strings ===\n";
if (isset($status['interned_strings_usage'])) {
    foreach ($status['interned_strings_usage'] as $k => $v) {
        echo "$k: $v\n";
    }
}

echo "\n=== Statistics ===\n";
if (isset($status['opcache_statistics'])) {
    foreach ($status['opcache_statistics'] as $k => $v) {
        echo "$k: $v\n";
    }
}

echo "\n=== Blacklist ===\n";
if (isset($config['blacklist'])) {
    print_r($config['blacklist']);
} else {
    echo "(none)\n";
}

echo "\n=== Test opcache_is_script_cached() ===\n";
$test_file = __FILE__;
echo "Testing: $test_file\n";
echo "Is cached: " . (opcache_is_script_cached($test_file) ? 'YES' : 'NO') . "\n";

echo "\n=== Trying manual compile ===\n";
$result = opcache_compile_file($test_file);
echo "opcache_compile_file returned: " . ($result ? 'true' : 'false') . "\n";
echo "After compile - is_cached: " . (opcache_is_script_cached($test_file) ? 'YES' : 'NO') . "\n";

// Check status again
$status2 = opcache_get_status(false);
echo "After compile - num_cached_scripts: " . $status2['opcache_statistics']['num_cached_scripts'] . "\n";
