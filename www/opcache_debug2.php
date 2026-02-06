<?php
header('Content-Type: text/plain');

echo "=== OPcache Debug ===\n";
echo "SAPI: " . php_sapi_name() . "\n";
echo "PHP: " . PHP_VERSION . " (" . (PHP_ZTS ? 'ZTS' : 'NTS') . ")\n";
echo "PID: " . getmypid() . "\n";
echo "Thread ID: " . (function_exists('zend_thread_id') ? zend_thread_id() : 'N/A') . "\n";
echo "time(): " . time() . "\n";
echo "REQUEST_TIME: " . ($_SERVER['REQUEST_TIME'] ?? 'N/A') . "\n";
echo "REQUEST_TIME_FLOAT: " . ($_SERVER['REQUEST_TIME_FLOAT'] ?? 'N/A') . "\n\n";

echo "=== INI values ===\n";
$keys = [
    'opcache.enable', 'opcache.enable_cli', 'opcache.memory_consumption',
    'opcache.interned_strings_buffer', 'opcache.max_accelerated_files',
    'opcache.validate_timestamps', 'opcache.file_update_protection',
    'opcache.use_cwd', 'opcache.file_cache', 'opcache.file_cache_only',
    'opcache.jit', 'opcache.jit_buffer_size',
];
foreach ($keys as $k) {
    echo "  $k = " . var_export(ini_get($k), true) . "\n";
}

echo "\n=== ZCG directives (opcache_get_configuration) ===\n";
$c = opcache_get_configuration();
if ($c) {
    foreach ($c['directives'] as $k => $v) {
        $display = is_bool($v) ? ($v ? 'true' : 'false') : var_export($v, true);
        echo "  $k = $display\n";
    }
}

echo "\n=== opcache_get_status ===\n";
$s = opcache_get_status(false);
if ($s) {
    echo "  opcache_enabled: " . ($s['opcache_enabled'] ? 'yes' : 'no') . "\n";
    foreach ($s['memory_usage'] as $k => $v) {
        echo "  memory.$k: $v\n";
    }
    foreach ($s['interned_strings_usage'] as $k => $v) {
        echo "  interned.$k: $v\n";
    }
    foreach ($s['opcache_statistics'] as $k => $v) {
        echo "  stat.$k: $v\n";
    }
    if (isset($s['jit'])) {
        echo "  jit.enabled: " . ($s['jit']['enabled'] ? 'yes' : 'no') . "\n";
        echo "  jit.on: " . ($s['jit']['on'] ? 'yes' : 'no') . "\n";
        echo "  jit.buffer_size: " . $s['jit']['buffer_size'] . "\n";
        echo "  jit.buffer_free: " . $s['jit']['buffer_free'] . "\n";
    }
}

echo "\n=== file_update_protection analysis ===\n";
$fup = $c['directives']['opcache.file_update_protection'] ?? 0;
$req_time = $_SERVER['REQUEST_TIME'] ?? time();
$file_mtime = filemtime(__FILE__);
echo "file_update_protection: $fup\n";
echo "request_time: $req_time\n";
echo "file_mtime: $file_mtime (" . date('Y-m-d H:i:s', $file_mtime) . ")\n";
echo "request_time - fup: " . ($req_time - $fup) . "\n";
echo "Would skip (file too new)? " . ($file_mtime > ($req_time - $fup) ? 'YES - TOO NEW!' : 'no') . "\n";

echo "\n=== Caching test ===\n";
echo "Before: cached=" . (opcache_is_script_cached(__FILE__) ? 'yes' : 'no') . "\n";
$r = opcache_compile_file(__FILE__);
echo "opcache_compile_file: " . ($r ? 'OK' : 'FAIL') . "\n";
echo "After: cached=" . (opcache_is_script_cached(__FILE__) ? 'yes' : 'no') . "\n";
$s2 = opcache_get_status(false);
echo "scripts=" . $s2['opcache_statistics']['num_cached_scripts'] . " misses=" . $s2['opcache_statistics']['misses'] . "\n";
