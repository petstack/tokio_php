<?php
echo "=== OPcache Diagnostic Debug ===\n\n";

// Get full configuration
$config = opcache_get_configuration();
$status = opcache_get_status(true);

echo "=== SAPI ===\n";
echo "php_sapi_name: " . php_sapi_name() . "\n";
echo "PHP_SAPI: " . PHP_SAPI . "\n";

echo "\n=== Critical INI Settings ===\n";
echo "opcache.enable: " . ini_get('opcache.enable') . "\n";
echo "opcache.enable_cli: " . ini_get('opcache.enable_cli') . "\n";
echo "opcache.validate_timestamps: " . ini_get('opcache.validate_timestamps') . "\n";
echo "opcache.file_update_protection: " . ini_get('opcache.file_update_protection') . "\n";
echo "opcache.max_file_size: " . ini_get('opcache.max_file_size') . "\n";
echo "opcache.memory_consumption: " . ini_get('opcache.memory_consumption') . "\n";

echo "\n=== Status Flags ===\n";
echo "opcache_enabled: " . ($status['opcache_enabled'] ? 'YES' : 'NO') . "\n";
echo "cache_full: " . ($status['cache_full'] ? 'YES' : 'NO') . "\n";
echo "restart_pending: " . ($status['restart_pending'] ? 'YES' : 'NO') . "\n";
echo "restart_in_progress: " . ($status['restart_in_progress'] ? 'YES' : 'NO') . "\n";

echo "\n=== Memory Status ===\n";
$mem = $status['memory_usage'];
// PHP 8.5 ZTS bug workaround: ZCG(accel_directives).memory_consumption is 0 on worker threads
// Use ini_get() which works correctly
$memory_consumption = (int)ini_get('opcache.memory_consumption') * 1024 * 1024;
$used_memory_raw = $mem['used_memory'];
$used_memory = $memory_consumption - $mem['free_memory'] - $mem['wasted_memory'];
if ($used_memory_raw < 0) {
    echo "used_memory: " . number_format($used_memory) . " bytes (fixed, raw: " . number_format($used_memory_raw) . ")\n";
} else {
    echo "used_memory: " . number_format($used_memory_raw) . " bytes\n";
}
echo "free_memory: " . number_format($mem['free_memory']) . " bytes\n";
echo "wasted_memory: " . number_format($mem['wasted_memory']) . " bytes\n";
$wasted_pct = $memory_consumption > 0 ? ($mem['wasted_memory'] / $memory_consumption) * 100 : 0;
echo "current_wasted_percentage: " . number_format($wasted_pct, 2) . "%\n";

echo "\n=== Statistics ===\n";
echo "num_cached_scripts: " . $status['opcache_statistics']['num_cached_scripts'] . "\n";
echo "num_cached_keys: " . $status['opcache_statistics']['num_cached_keys'] . "\n";
echo "max_cached_keys: " . $status['opcache_statistics']['max_cached_keys'] . "\n";
echo "hits: " . $status['opcache_statistics']['hits'] . "\n";
echo "misses: " . $status['opcache_statistics']['misses'] . "\n";
echo "blacklist_misses: " . $status['opcache_statistics']['blacklist_misses'] . "\n";
echo "oom_restarts: " . $status['opcache_statistics']['oom_restarts'] . "\n";
echo "hash_restarts: " . $status['opcache_statistics']['hash_restarts'] . "\n";
echo "manual_restarts: " . $status['opcache_statistics']['manual_restarts'] . "\n";

echo "\n=== Interned Strings ===\n";
if (isset($status['interned_strings_usage'])) {
    echo "buffer_size: " . number_format($status['interned_strings_usage']['buffer_size']) . "\n";
    echo "used_memory: " . number_format($status['interned_strings_usage']['used_memory']) . "\n";
    echo "free_memory: " . number_format($status['interned_strings_usage']['free_memory']) . "\n";
    echo "number_of_strings: " . $status['interned_strings_usage']['number_of_strings'] . "\n";
} else {
    echo "(not available)\n";
}

echo "\n=== Test: Direct opcache_compile_file() ===\n";
// Test compiling this file
$this_file = __FILE__;
echo "File: $this_file\n";
echo "Realpath: " . realpath($this_file) . "\n";
echo "File exists: " . (file_exists($this_file) ? 'YES' : 'NO') . "\n";

// Check stat
$stat = stat($this_file);
echo "File mtime: " . date('Y-m-d H:i:s', $stat['mtime']) . "\n";
echo "File size: " . $stat['size'] . " bytes\n";

// Try to compile
echo "\nBefore compile:\n";
echo "  is_cached: " . (opcache_is_script_cached($this_file) ? 'YES' : 'NO') . "\n";
$status_before = opcache_get_status(false);
echo "  num_cached: " . $status_before['opcache_statistics']['num_cached_scripts'] . "\n";

$result = opcache_compile_file($this_file);
echo "\nCompile result: " . ($result ? 'SUCCESS' : 'FAILURE') . "\n";

echo "\nAfter compile:\n";
echo "  is_cached: " . (opcache_is_script_cached($this_file) ? 'YES' : 'NO') . "\n";
$status_after = opcache_get_status(false);
echo "  num_cached: " . $status_after['opcache_statistics']['num_cached_scripts'] . "\n";
echo "  misses: " . $status_after['opcache_statistics']['misses'] . "\n";

echo "\n=== Cached Scripts ===\n";
if (isset($status['scripts']) && is_array($status['scripts'])) {
    if (empty($status['scripts'])) {
        echo "(none)\n";
    } else {
        foreach ($status['scripts'] as $path => $info) {
            echo "- $path\n";
            echo "    hits: " . $info['hits'] . "\n";
            echo "    memory: " . number_format($info['memory_consumption']) . " bytes\n";
            if (isset($info['timestamp'])) {
                echo "    timestamp: " . date('Y-m-d H:i:s', $info['timestamp']) . "\n";
            }
        }
    }
}

echo "\n=== JIT Status ===\n";
if (isset($status['jit'])) {
    echo "enabled: " . ($status['jit']['enabled'] ? 'YES' : 'NO') . "\n";
    echo "on: " . ($status['jit']['on'] ? 'YES' : 'NO') . "\n";
    echo "kind: " . ($status['jit']['kind'] ?? 'N/A') . "\n";
    echo "buffer_size: " . number_format($status['jit']['buffer_size'] ?? 0) . "\n";
    echo "buffer_free: " . number_format($status['jit']['buffer_free'] ?? 0) . "\n";
} else {
    echo "(JIT not available)\n";
}
