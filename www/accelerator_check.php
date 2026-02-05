<?php
echo "=== Accelerator Check ===\n";

// Get full status
$status = opcache_get_status(true);

echo "opcache_enabled: " . ($status['opcache_enabled'] ? 'YES' : 'NO') . "\n";
echo "cache_full: " . ($status['cache_full'] ? 'YES' : 'NO') . "\n";
echo "restart_pending: " . ($status['restart_pending'] ? 'YES' : 'NO') . "\n";
echo "restart_in_progress: " . ($status['restart_in_progress'] ? 'YES' : 'NO') . "\n";

echo "\n=== Memory ===\n";
echo "used_memory: " . number_format($status['memory_usage']['used_memory']) . "\n";
echo "free_memory: " . number_format($status['memory_usage']['free_memory']) . "\n";
echo "wasted_memory: " . number_format($status['memory_usage']['wasted_memory']) . "\n";

echo "\n=== Statistics ===\n";
echo "num_cached_scripts: " . $status['opcache_statistics']['num_cached_scripts'] . "\n";
echo "num_cached_keys: " . $status['opcache_statistics']['num_cached_keys'] . "\n";
echo "max_cached_keys: " . $status['opcache_statistics']['max_cached_keys'] . "\n";
echo "hits: " . $status['opcache_statistics']['hits'] . "\n";
echo "misses: " . $status['opcache_statistics']['misses'] . "\n";
echo "oom_restarts: " . $status['opcache_statistics']['oom_restarts'] . "\n";
echo "hash_restarts: " . $status['opcache_statistics']['hash_restarts'] . "\n";

echo "\n=== Interned Strings ===\n";
if (isset($status['interned_strings_usage'])) {
    echo "buffer_size: " . number_format($status['interned_strings_usage']['buffer_size']) . "\n";
    echo "used_memory: " . number_format($status['interned_strings_usage']['used_memory']) . "\n";
    echo "free_memory: " . number_format($status['interned_strings_usage']['free_memory']) . "\n";
    echo "number_of_strings: " . $status['interned_strings_usage']['number_of_strings'] . "\n";
}

echo "\n=== Scripts (first 5) ===\n";
if (isset($status['scripts']) && is_array($status['scripts'])) {
    $i = 0;
    foreach ($status['scripts'] as $path => $info) {
        if ($i++ >= 5) {
            echo "... and " . (count($status['scripts']) - 5) . " more\n";
            break;
        }
        echo "- $path\n";
    }
    if (empty($status['scripts'])) {
        echo "(no scripts cached)\n";
    }
}

echo "\n=== Test: include a file and check if cached ===\n";
// Include this file itself
$this_file = __FILE__;
echo "This file: $this_file\n";
echo "Realpath: " . realpath($this_file) . "\n";
echo "Is cached before: " . (opcache_is_script_cached($this_file) ? 'YES' : 'NO') . "\n";

// Re-check after request
$status2 = opcache_get_status(false);
echo "num_cached_scripts after: " . $status2['opcache_statistics']['num_cached_scripts'] . "\n";
