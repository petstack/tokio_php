<?php
header('Content-Type: text/plain');
// Check if extensions are loaded correctly
echo "Loaded extensions:\n";
$exts = get_loaded_extensions();
foreach (['opcache', 'sodium', 'session'] as $ext) {
    echo "  $ext: " . (in_array($ext, $exts) ? "YES" : "NO") . "\n";
}

echo "\nOPcache extension info:\n";
$funcs = ['opcache_get_status', 'opcache_get_configuration', 'opcache_is_script_cached'];
foreach ($funcs as $f) {
    echo "  $f: " . (function_exists($f) ? "YES" : "NO") . "\n";
}

echo "\nINI settings (ini_get vs opcache config):\n";
$ini_keys = ['opcache.enable', 'opcache.memory_consumption', 'opcache.enable_cli'];
$oc = opcache_get_configuration();
foreach ($ini_keys as $k) {
    echo "  $k:\n";
    echo "    ini_get: " . ini_get($k) . "\n";
    echo "    oc_cfg:  " . ($oc['directives'][$k] ?? 'N/A') . "\n";
}
