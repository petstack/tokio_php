<?php
header('Content-Type: text/plain');
echo "php.ini loaded: " . php_ini_loaded_file() . "\n";
echo "Additional ini: " . php_ini_scanned_files() . "\n";
echo "opcache.memory_consumption: " . ini_get('opcache.memory_consumption') . "\n";
echo "opcache.enable: " . ini_get('opcache.enable') . "\n";
