<?php
header('Content-Type: text/plain');

// Capture phpinfo opcache section
ob_start();
phpinfo(INFO_MODULES);
$info = ob_get_clean();

// Extract opcache section
if (preg_match('/Zend OPcache.*?(?=<h2|$)/s', $info, $matches)) {
    // Strip HTML tags for cleaner output
    $opcache_info = strip_tags($matches[0]);
    $opcache_info = html_entity_decode($opcache_info);
    $opcache_info = preg_replace('/\n{3,}/', "\n\n", $opcache_info);
    echo $opcache_info;
} else {
    echo "OPcache section not found in phpinfo\n";

    // Show raw modules list
    echo "\n=== Loaded Modules ===\n";
    print_r(get_loaded_extensions());
    echo "\n=== Zend Extensions ===\n";
    print_r(get_loaded_extensions(true));
}
