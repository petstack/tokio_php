<?php
// Check if the issue is with the callback or the value lookup
$config = opcache_get_configuration();

echo "=== Comparing directive values ===\n\n";

$directives = [
    'opcache.memory_consumption' => 'OnUpdateMemoryConsumption',
    'opcache.interned_strings_buffer' => 'OnUpdateInternedStringsBuffer', 
    'opcache.max_accelerated_files' => 'OnUpdateMaxAcceleratedFiles',
    'opcache.enable' => 'OnUpdateBool',
    'opcache.enable_cli' => 'OnUpdateBool',
];

foreach ($directives as $name => $callback) {
    $ini = ini_get($name);
    $directive = $config['directives'][$name] ?? 'N/A';
    $match = ($ini == $directive || ($directive === true && $ini == '1') || ($directive === false && $ini == '0')) ? 'OK' : 'MISMATCH';
    
    printf("%-35s: ini_get=%-10s directive=%-15s [%s]\n", $name, var_export($ini, true), var_export($directive, true), $match);
}

echo "\n=== Raw directive values ===\n";
echo "memory_consumption raw: " . var_export($config['directives']['opcache.memory_consumption'], true) . "\n";
echo "type: " . gettype($config['directives']['opcache.memory_consumption']) . "\n";

// Check if 0 is actually 0 or something else
$mem = $config['directives']['opcache.memory_consumption'];
if ($mem === 0) {
    echo "memory_consumption is exactly integer 0\n";
} elseif ($mem === '0') {
    echo "memory_consumption is string '0'\n";
} elseif ($mem === false) {
    echo "memory_consumption is boolean false\n";
} elseif ($mem === null) {
    echo "memory_consumption is null\n";
}
