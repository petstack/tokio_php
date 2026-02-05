<?php
// Check configuration directives
echo "=== get_cfg_var test ===\n";
$cfg = get_cfg_var('opcache.memory_consumption');
echo "get_cfg_var('opcache.memory_consumption'): " . var_export($cfg, true) . "\n";
echo "Type: " . gettype($cfg) . "\n";

echo "\n=== ini_get_all for opcache ===\n";
$opcache_ini = ini_get_all('zend opcache', true);
if (isset($opcache_ini['opcache.memory_consumption'])) {
    echo "opcache.memory_consumption:\n";
    print_r($opcache_ini['opcache.memory_consumption']);
} else {
    echo "opcache.memory_consumption not found in ini_get_all\n";
}

echo "\n=== Check if interned_strings_buffer works the same ===\n";
$cfg2 = get_cfg_var('opcache.interned_strings_buffer');
echo "get_cfg_var('opcache.interned_strings_buffer'): " . var_export($cfg2, true) . "\n";

if (isset($opcache_ini['opcache.interned_strings_buffer'])) {
    echo "opcache.interned_strings_buffer:\n";
    print_r($opcache_ini['opcache.interned_strings_buffer']);
}
