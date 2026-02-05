<?php
echo "=== Extension Debug ===\n";

echo "\n=== All Extensions ===\n";
$exts = get_loaded_extensions();
sort($exts);
foreach ($exts as $ext) {
    echo "- $ext\n";
}

echo "\n=== Zend Extensions ===\n";
$zend_exts = get_loaded_extensions(true);
foreach ($zend_exts as $ext) {
    echo "- $ext\n";
}

echo "\n=== extension_loaded() checks ===\n";
echo "Zend OPcache: " . (extension_loaded('Zend OPcache') ? 'YES' : 'NO') . "\n";
echo "opcache: " . (extension_loaded('opcache') ? 'YES' : 'NO') . "\n";
echo "OPcache: " . (extension_loaded('OPcache') ? 'YES' : 'NO') . "\n";

echo "\n=== ini_get_all for different names ===\n";
$names = ['Zend OPcache', 'opcache', 'OPcache', 'zend opcache'];
foreach ($names as $name) {
    $result = @ini_get_all($name, false);
    echo "$name: " . ($result === false ? 'FALSE' : count($result) . ' entries') . "\n";
}

echo "\n=== opcache functions exist ===\n";
$funcs = ['opcache_get_status', 'opcache_compile_file', 'opcache_invalidate', 'opcache_reset'];
foreach ($funcs as $func) {
    echo "$func: " . (function_exists($func) ? 'YES' : 'NO') . "\n";
}

echo "\n=== Direct INI checks ===\n";
echo "opcache.enable: " . ini_get('opcache.enable') . "\n";
echo "opcache.enable_cli: " . ini_get('opcache.enable_cli') . "\n";
echo "opcache.memory_consumption: " . ini_get('opcache.memory_consumption') . "\n";
