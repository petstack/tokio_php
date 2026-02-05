<?php
header('Content-Type: text/plain');
// Multiple requests to same file should show OPcache hits
echo "SAPI: " . php_sapi_name() . "\n";
echo "OPcache enabled: " . (opcache_is_script_cached(__FILE__) ? "YES" : "NO") . "\n";
$s = opcache_get_status(false);
if ($s) {
    echo "opcache_enabled: " . ($s['opcache_enabled'] ? 'true' : 'false') . "\n";
    echo "num_cached_scripts: " . $s['opcache_statistics']['num_cached_scripts'] . "\n";
    echo "hits: " . $s['opcache_statistics']['hits'] . "\n";
    echo "misses: " . $s['opcache_statistics']['misses'] . "\n";
} else {
    echo "opcache_get_status() returned false\n";
}
