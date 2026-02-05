<?php
echo "=== Loaded Extensions ===\n";
print_r(get_loaded_extensions());

echo "\n=== Zend Extensions ===\n";
print_r(get_loaded_extensions(true));

echo "\n=== OPcache Info ===\n";
if (extension_loaded('Zend OPcache')) {
    echo "OPcache extension loaded: YES\n";
} else {
    echo "OPcache extension loaded: NO\n";
}

echo "\n=== php_sapi_name ===\n";
echo php_sapi_name() . "\n";

echo "\n=== OPcache accelerator_enabled check ===\n";
$status = opcache_get_status(false);
echo "accelerator_enabled: " . ($status['opcache_enabled'] ? 'YES' : 'NO') . "\n";

echo "\n=== Memory model ===\n";
$config = opcache_get_configuration();
echo "preferred_memory_model: '" . $config['directives']['opcache.preferred_memory_model'] . "'\n";

echo "\n=== SHM check via /proc ===\n";
if (file_exists('/proc/self/maps')) {
    $maps = file_get_contents('/proc/self/maps');
    if (preg_match_all('/([0-9a-f]+)-([0-9a-f]+).*?(\/dev\/shm|SYSV|deleted)/i', $maps, $matches)) {
        foreach ($matches[0] as $match) {
            echo $match . "\n";
        }
    } else {
        echo "No SHM mappings found\n";
    }
} else {
    echo "/proc/self/maps not available\n";
}
