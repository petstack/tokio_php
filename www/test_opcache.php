<?php
error_reporting(E_ALL);
ini_set('display_errors', 1);

echo "=== OPcache Test ===\n\n";
echo "SAPI: " . php_sapi_name() . "\n";
echo "opcache.enable: " . ini_get('opcache.enable') . "\n";
echo "opcache.enable_cli: " . ini_get('opcache.enable_cli') . "\n";
echo "opcache.memory_consumption: " . ini_get('opcache.memory_consumption') . "\n\n";

echo "Checking if function exists: " . (function_exists('opcache_get_status') ? "YES" : "NO") . "\n\n";

if (function_exists('opcache_get_status')) {
    $status = opcache_get_status(false);
    if ($status === false) {
        echo "opcache_get_status() returned false\n";
    } else {
        echo "opcache_enabled: " . ($status['opcache_enabled'] ? "YES" : "NO") . "\n";
        if (isset($status['memory_usage'])) {
            echo "used_memory: " . number_format($status['memory_usage']['used_memory']) . " bytes\n";
            echo "free_memory: " . number_format($status['memory_usage']['free_memory']) . " bytes\n";
        }
        if (isset($status['opcache_statistics'])) {
            echo "num_cached_scripts: " . $status['opcache_statistics']['num_cached_scripts'] . "\n";
            echo "hits: " . $status['opcache_statistics']['hits'] . "\n";
            echo "misses: " . $status['opcache_statistics']['misses'] . "\n";
        }
    }
}
