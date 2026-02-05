<?php
// Check SAPI info
echo "=== SAPI Info ===\n";
echo "php_sapi_name(): " . php_sapi_name() . "\n";
echo "PHP_SAPI constant: " . PHP_SAPI . "\n";

// Check if we can access ini_entries-related info
echo "\n=== INI files loaded ===\n";
echo "php_ini_loaded_file(): " . var_export(php_ini_loaded_file(), true) . "\n";
echo "php_ini_scanned_files():\n" . php_ini_scanned_files() . "\n";

// Check specific settings that would come from sapi ini_entries
echo "\n=== Settings from SAPI ini_entries ===\n";
echo "html_errors: " . ini_get('html_errors') . "\n";
echo "implicit_flush: " . ini_get('implicit_flush') . "\n";
echo "output_buffering: " . ini_get('output_buffering') . "\n";
