<?php
$status = opcache_get_status(true);
$enabled = $status && $status['opcache_enabled'];
?>
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>OPcache Status - tokio_php</title>
    <style>
        * { box-sizing: border-box; margin: 0; padding: 0; }
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; max-width: 720px; margin: 40px auto; padding: 20px; background: #fff; color: #333; line-height: 1.6; }
        h1 { font-size: 24px; font-weight: 600; margin-bottom: 8px; }
        .subtitle { color: #666; margin-bottom: 32px; }
        .section { margin-bottom: 28px; }
        .section-title { font-size: 12px; font-weight: 600; color: #999; text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 12px; }
        a { color: #0066cc; text-decoration: none; }
        a:hover { text-decoration: underline; }
        code { background: #f5f5f5; padding: 2px 6px; border-radius: 3px; font-size: 13px; font-family: 'SF Mono', Consolas, monospace; }
        .card { background: #fafafa; border: 1px solid #e8e8e8; border-radius: 8px; padding: 16px; margin-bottom: 16px; }
        .nav { margin-bottom: 24px; padding-bottom: 16px; border-bottom: 1px solid #eee; }
        .row { display: flex; justify-content: space-between; padding: 8px 0; border-bottom: 1px solid #f0f0f0; }
        .row:last-child { border-bottom: none; }
        .label { color: #666; }
        .value { font-weight: 500; }
        .ok { color: #22863a; }
        .warn { color: #b08800; }
        .bar { height: 8px; background: #e8e8e8; border-radius: 4px; overflow: hidden; margin-top: 4px; }
        .bar-fill { height: 100%; background: #0066cc; }
        .bar-fill.warn { background: #ffc107; }
        .bar-fill.danger { background: #dc3545; }

        .tabs { display: flex; gap: 6px; flex-wrap: wrap; margin-bottom: 12px; }
        .tab { padding: 6px 12px; background: #f5f5f5; border: 1px solid #e8e8e8; border-radius: 6px; font-family: 'SF Mono', Consolas, monospace; font-size: 13px; color: #666; cursor: pointer; transition: all 0.15s ease; }
        .tab:hover { background: #eee; color: #333; }
        .tab.active { background: #0066cc; border-color: #0066cc; color: #fff; }
        .tab-content { display: none; background: #fafafa; border: 1px solid #e8e8e8; border-radius: 8px; overflow: hidden; }
        .tab-content.active { display: block; }
        .tab-header { padding: 10px 14px; background: #f0f0f0; border-bottom: 1px solid #e8e8e8; font-size: 12px; color: #666; }
        .tab-body { max-height: 320px; overflow-y: auto; }
        .tab-empty { padding: 24px; text-align: center; color: #999; font-size: 13px; }
        .kv-table { width: 100%; border-collapse: collapse; font-size: 13px; }
        .kv-table tr { border-bottom: 1px solid #eee; }
        .kv-table tr:last-child { border-bottom: none; }
        .kv-table tr:hover { background: #f5f5f5; }
        .kv-table td { padding: 8px 14px; vertical-align: top; }
        .kv-table .key { width: 35%; font-family: 'SF Mono', Consolas, monospace; color: #0066cc; word-break: break-all; }
        .kv-table .val { color: #333; word-break: break-all; font-family: 'SF Mono', Consolas, monospace; }
        .kv-table .val-string { color: #22863a; }
        .kv-table .val-number { color: #005cc5; }
        .kv-table .val-array { color: #6f42c1; }
    </style>
</head>
<body>
    <div class="nav"><a href="/">Home</a></div>

    <h1>OPcache Status</h1>
    <p class="subtitle">Real-time cache statistics</p>

    <?php if (!$enabled): ?>
    <div class="card" style="background: #f8d7da; border-color: #f5c6cb; color: #721c24;">
        OPcache is not enabled
    </div>
    <?php else:
        $stats = $status['opcache_statistics'];
        $memory = $status['memory_usage'];
        $hitRate = $stats['hits'] > 0 ? round($stats['hits'] / ($stats['hits'] + $stats['misses']) * 100, 1) : 0;
        $memUsed = round($memory['used_memory'] / 1024 / 1024, 1);
        $memFree = round($memory['free_memory'] / 1024 / 1024, 1);
        $memTotal = $memUsed + $memFree;
        $memPct = $memTotal > 0 ? round($memUsed / $memTotal * 100) : 0;
    ?>

    <div class="section">
        <div class="section-title">Statistics</div>
        <div class="card">
            <div class="row">
                <span class="label">Cached Scripts</span>
                <span class="value"><?= number_format($stats['num_cached_scripts']) ?></span>
            </div>
            <div class="row">
                <span class="label">Hits</span>
                <span class="value ok"><?= number_format($stats['hits']) ?></span>
            </div>
            <div class="row">
                <span class="label">Misses</span>
                <span class="value"><?= number_format($stats['misses']) ?></span>
            </div>
            <div class="row">
                <span class="label">Hit Rate</span>
                <span class="value <?= $hitRate > 90 ? 'ok' : 'warn' ?>"><?= $hitRate ?>%</span>
            </div>
        </div>
    </div>

    <div class="section">
        <div class="section-title">Memory</div>
        <div class="card">
            <div class="row">
                <span class="label">Used</span>
                <span class="value"><?= $memUsed ?> MB</span>
            </div>
            <div class="row">
                <span class="label">Free</span>
                <span class="value"><?= $memFree ?> MB</span>
            </div>
            <div class="bar">
                <div class="bar-fill <?= $memPct > 90 ? 'danger' : ($memPct > 70 ? 'warn' : '') ?>" style="width: <?= $memPct ?>%"></div>
            </div>
            <div style="font-size: 12px; color: #999; margin-top: 4px;"><?= $memPct ?>% used</div>
        </div>
    </div>

    <?php if (isset($status['jit']) && $status['jit']['enabled']):
        $jit = $status['jit'];
        $jitUsed = round(($jit['buffer_size'] - $jit['buffer_free']) / 1024 / 1024, 1);
        $jitTotal = round($jit['buffer_size'] / 1024 / 1024, 1);
        $jitPct = round($jitUsed / $jitTotal * 100);
    ?>
    <div class="section">
        <div class="section-title">JIT</div>
        <div class="card">
            <div class="row">
                <span class="label">Status</span>
                <span class="value ok"><?= $jit['on'] ? 'Active' : 'Standby' ?></span>
            </div>
            <div class="row">
                <span class="label">Kind</span>
                <span class="value"><?= $jit['kind'] ?></span>
            </div>
            <div class="row">
                <span class="label">Buffer</span>
                <span class="value"><?= $jitUsed ?> / <?= $jitTotal ?> MB</span>
            </div>
            <div class="bar">
                <div class="bar-fill" style="width: <?= $jitPct ?>%"></div>
            </div>
        </div>
    </div>
    <?php endif; ?>

    <?php if (isset($status['scripts']) && count($status['scripts']) > 0):
        $scripts = $status['scripts'];
        usort($scripts, fn($a, $b) => $b['hits'] - $a['hits']);
        $scripts = array_slice($scripts, 0, 10);
    ?>
    <div class="section">
        <div class="section-title">Top Scripts by Hits</div>
        <div class="card">
            <?php foreach ($scripts as $s): ?>
            <div class="row">
                <span class="label"><code><?= basename($s['full_path']) ?></code></span>
                <span class="value"><?= number_format($s['hits']) ?> hits</span>
            </div>
            <?php endforeach; ?>
        </div>
    </div>
    <?php endif; ?>

    <?php endif; ?>

    <div class="section">
        <div class="section-title">Superglobals</div>
        <div class="tabs">
            <div class="tab active" data-tab="get">$_GET</div>
            <div class="tab" data-tab="post">$_POST</div>
            <div class="tab" data-tab="server">$_SERVER</div>
            <div class="tab" data-tab="cookie">$_COOKIE</div>
            <div class="tab" data-tab="files">$_FILES</div>
            <div class="tab" data-tab="request">$_REQUEST</div>
        </div>
        <?php include __DIR__ . '/_tabs.php'; ?>
    </div>

    <script>
        document.querySelectorAll('.tab').forEach(tab => {
            tab.addEventListener('click', () => {
                document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
                document.querySelectorAll('.tab-content').forEach(c => c.classList.remove('active'));
                tab.classList.add('active');
                document.getElementById('tab-' + tab.dataset.tab).classList.add('active');
            });
        });
        document.querySelector('.tab.active').click();
    </script>
<div style="margin-top: 32px; padding-top: 16px; border-top: 1px solid #eee; font-size: 12px; color: #999;"><?= number_format((microtime(true) - $_SERVER['REQUEST_TIME_FLOAT']) * 1000, 2) ?> ms</div>
</body>
</html>
