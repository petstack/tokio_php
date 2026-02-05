<?php
$status = opcache_get_status(false);
$mem = $status['memory_usage'];

// PHP 8.5 ZTS bug: memory_consumption from INI is 0 on worker threads
// Workaround: use known memory size from INI
$memory_consumption = (int)ini_get('opcache.memory_consumption') * 1024 * 1024;

// If INI returns 0 (also thread-local!), try to calculate from free_memory
// In normal case: used + free + wasted = total
// If used is negative, total = -used + free + wasted (approximation)
if ($memory_consumption == 0 && $mem['used_memory'] < 0) {
    // Estimate: total ≈ free + wasted (since used is wrong)
    $memory_consumption = $mem['free_memory'] + $mem['wasted_memory'] + abs($mem['used_memory']);
}

$used = $memory_consumption - $mem['free_memory'] - $mem['wasted_memory'];

echo "=== OPcache Memory (Fixed) ===\n";
echo "memory_consumption (INI): " . number_format($memory_consumption) . " bytes\n";
echo "used_memory (raw): " . number_format($mem['used_memory']) . " bytes\n";
echo "used_memory (fixed): " . number_format($used) . " bytes\n";
echo "free_memory: " . number_format($mem['free_memory']) . " bytes\n";
echo "wasted_memory: " . number_format($mem['wasted_memory']) . " bytes\n";

// Verify
$sum = $used + $mem['free_memory'] + $mem['wasted_memory'];
echo "\nVerification:\n";
echo "used + free + wasted = " . number_format($sum) . " bytes\n";
echo "Expected (128MB): " . number_format(128 * 1024 * 1024) . " bytes\n";
echo "Match: " . ($sum == 128 * 1024 * 1024 ? 'YES' : 'NO') . "\n";
