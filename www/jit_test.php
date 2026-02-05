<?php
echo "=== JIT Test ===\n\n";

$status = opcache_get_status(false);
$jit = $status['jit'] ?? null;

if (!$jit) {
    echo "JIT not available\n";
    exit(1);
}

echo "=== JIT Configuration ===\n";
echo "opcache.jit: " . ini_get('opcache.jit') . "\n";
echo "opcache.jit_buffer_size: " . ini_get('opcache.jit_buffer_size') . "\n";

echo "\n=== JIT Status (before) ===\n";
echo "enabled: " . ($jit['enabled'] ? 'YES' : 'NO') . "\n";
echo "on: " . ($jit['on'] ? 'YES' : 'NO') . "\n";
echo "kind: " . $jit['kind'] . "\n";
echo "opt_level: " . ($jit['opt_level'] ?? 'N/A') . "\n";
echo "opt_flags: " . ($jit['opt_flags'] ?? 'N/A') . "\n";
echo "buffer_size: " . number_format($jit['buffer_size']) . " bytes\n";
echo "buffer_free: " . number_format($jit['buffer_free']) . " bytes\n";
$used_before = $jit['buffer_size'] - $jit['buffer_free'];
echo "buffer_used: " . number_format($used_before) . " bytes\n";

// CPU-intensive function to trigger JIT
function fibonacci($n) {
    if ($n <= 1) return $n;
    return fibonacci($n - 1) + fibonacci($n - 2);
}

function compute_intensive() {
    $sum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += $i * $i;
        $sum = $sum % 1000000007;
    }
    return $sum;
}

echo "\n=== Running CPU-intensive code ===\n";
$start = microtime(true);

// Run multiple times to trigger JIT compilation
for ($run = 0; $run < 5; $run++) {
    $result1 = fibonacci(25);
    $result2 = compute_intensive();
}

$elapsed = (microtime(true) - $start) * 1000;
echo "fibonacci(25) = $result1\n";
echo "compute_intensive() = $result2\n";
echo "Time: " . number_format($elapsed, 2) . " ms\n";

// Check JIT status after
$status2 = opcache_get_status(false);
$jit2 = $status2['jit'];

echo "\n=== JIT Status (after) ===\n";
echo "buffer_size: " . number_format($jit2['buffer_size']) . " bytes\n";
echo "buffer_free: " . number_format($jit2['buffer_free']) . " bytes\n";
$used_after = $jit2['buffer_size'] - $jit2['buffer_free'];
echo "buffer_used: " . number_format($used_after) . " bytes\n";

$jit_compiled = $used_after - $used_before;
echo "\n=== JIT Compilation Result ===\n";
if ($jit_compiled > 0) {
    echo "NEW JIT code compiled: " . number_format($jit_compiled) . " bytes\n";
    echo "JIT is WORKING!\n";
} else {
    echo "No new JIT code (already compiled or JIT not triggered)\n";
}

// Show JIT debug info if available
if (function_exists('opcache_get_status')) {
    $full_status = opcache_get_status(true);
    if (isset($full_status['jit']['buffer_size'])) {
        $pct_used = (1 - $full_status['jit']['buffer_free'] / $full_status['jit']['buffer_size']) * 100;
        echo "Buffer utilization: " . number_format($pct_used, 2) . "%\n";
    }
}
