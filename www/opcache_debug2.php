<?php
header('Content-Type: text/plain');

echo "=== OPcache Detailed Debug ===\n\n";

// Get all INI values
$all_ini = ini_get_all('zend opcache');
if (empty($all_ini)) {
    echo "No OPcache INI entries found!\n";
} else {
    echo "OPcache INI entries:\n";
    foreach ($all_ini as $key => $data) {
        echo "  $key:\n";
        echo "    global_value: " . var_export($data['global_value'], true) . "\n";
        echo "    local_value: " . var_export($data['local_value'], true) . "\n";
        echo "    access: " . $data['access'] . "\n";
    }
}

echo "\n=== Check if shm was initialized ===\n";
$status = @opcache_get_status(false);
if ($status === false) {
    echo "opcache_get_status() returned FALSE\n";
} else {
    echo "opcache_enabled: " . ($status['opcache_enabled'] ? 'true' : 'false') . "\n";

    // Memory usage shows if SHM is working
    $mem = $status['memory_usage'] ?? [];
    echo "\nmemory_usage:\n";
    foreach ($mem as $k => $v) {
        echo "  $k: $v\n";
    }

    // Check SHM-related internals
    $interned = $status['interned_strings_usage'] ?? [];
    echo "\ninterned_strings_usage:\n";
    foreach ($interned as $k => $v) {
        echo "  $k: $v\n";
    }
}

echo "\n=== OPcache configuration directives ===\n";
$config = opcache_get_configuration();
$directives = $config['directives'] ?? [];
foreach ($directives as $k => $v) {
    echo "$k: " . var_export($v, true) . "\n";
}
