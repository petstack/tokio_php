<?php
echo "=== PHP INI Files ===\n";
echo "Loaded php.ini: " . php_ini_loaded_file() . "\n";
echo "Scanned dir: " . php_ini_scanned_files() . "\n\n";

echo "=== OPcache INI from get_ini_file() ===\n";
$ini_file = php_ini_loaded_file();
if ($ini_file && file_exists($ini_file)) {
    $content = file_get_contents($ini_file);
    if (preg_match('/opcache\.memory/', $content)) {
        echo "Found opcache settings in main ini\n";
    } else {
        echo "No opcache settings in main ini\n";
    }
}

echo "\n=== Configuration via php_ini_scanned_files ===\n";
$scanned = php_ini_scanned_files();
if ($scanned) {
    foreach (explode(',', $scanned) as $file) {
        $file = trim($file);
        if ($file && file_exists($file)) {
            echo "File: $file\n";
            $content = file_get_contents($file);
            if (preg_match('/opcache\.memory_consumption\s*=\s*(\d+)/', $content, $m)) {
                echo "  -> opcache.memory_consumption = {$m[1]}\n";
            }
        }
    }
} else {
    echo "No scanned files\n";
}

echo "\n=== get_cfg_var() vs ini_get() ===\n";
echo "get_cfg_var('opcache.memory_consumption'): " . var_export(get_cfg_var('opcache.memory_consumption'), true) . "\n";
echo "ini_get('opcache.memory_consumption'): " . var_export(ini_get('opcache.memory_consumption'), true) . "\n";
echo "get_cfg_var('opcache.enable'): " . var_export(get_cfg_var('opcache.enable'), true) . "\n";
echo "ini_get('opcache.enable'): " . var_export(ini_get('opcache.enable'), true) . "\n";
