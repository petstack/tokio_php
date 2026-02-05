<?php
echo "=== SAPI Check ===\n";
echo "php_sapi_name(): " . php_sapi_name() . "\n";
echo "PHP_SAPI constant: " . PHP_SAPI . "\n";

// Check OPcache enabled
$enabled = ini_get('opcache.enable');
echo "\nopcache.enable (ini): " . ($enabled ? $enabled : '(empty)') . "\n";

// Get all opcache INI
echo "\n=== OPcache INI Settings ===\n";
foreach (ini_get_all('Zend OPcache', false) as $key => $value) {
    if (strpos($key, 'opcache') === 0) {
        echo "$key = " . ($value === '' ? '(empty)' : $value) . "\n";
    }
}

// Check if we're in the ZCG(enabled) state
$status = opcache_get_status(false);
echo "\n=== OPcache Status ===\n";
echo "opcache_enabled: " . ($status['opcache_enabled'] ? 'YES' : 'NO') . "\n";
