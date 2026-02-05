<?php
$status = opcache_get_status(true);
$config = opcache_get_configuration();

echo "=== OPcache Status ===\n";
echo "opcache_enabled: " . ($status['opcache_enabled'] ? 'true' : 'false') . "\n";
echo "file_cache_only: " . ($config['directives']['opcache.file_cache_only'] ? 'true' : 'false') . "\n";
echo "file_cache: " . var_export($config['directives']['opcache.file_cache'], true) . "\n";

echo "\n=== Memory Configuration ===\n";
echo "memory_consumption (directive): " . $config['directives']['opcache.memory_consumption'] . "\n";
echo "memory_consumption (ini_get): " . ini_get('opcache.memory_consumption') . "\n";

echo "\n=== Shared Memory Status ===\n";
if (isset($status['memory_usage'])) {
    echo "used_memory: " . $status['memory_usage']['used_memory'] . "\n";
    echo "free_memory: " . $status['memory_usage']['free_memory'] . "\n";
    echo "wasted_memory: " . $status['memory_usage']['wasted_memory'] . "\n";
    
    $total = $status['memory_usage']['used_memory'] + $status['memory_usage']['free_memory'];
    echo "calculated_total: " . $total . " (" . round($total / 1024 / 1024, 2) . " MB)\n";
} else {
    echo "memory_usage not available\n";
}

echo "\n=== Interned Strings ===\n";
if (isset($status['interned_strings_usage'])) {
    print_r($status['interned_strings_usage']);
} else {
    echo "interned_strings_usage not available\n";
}

echo "\n=== Scripts ===\n";
if (isset($status['scripts']) && count($status['scripts']) > 0) {
    echo "Cached scripts:\n";
    foreach ($status['scripts'] as $path => $info) {
        echo "  - " . $path . "\n";
    }
} else {
    echo "No scripts cached\n";
}
