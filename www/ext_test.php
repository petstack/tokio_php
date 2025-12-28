<?php
$extLoaded = extension_loaded('tokio_sapi');
$functions = [
    'tokio_request_id' => function_exists('tokio_request_id'),
    'tokio_worker_id' => function_exists('tokio_worker_id'),
    'tokio_server_info' => function_exists('tokio_server_info'),
];
$serverInfo = function_exists('tokio_server_info') ? tokio_server_info() : null;
$requestId = function_exists('tokio_request_id') ? tokio_request_id() : null;
$workerId = function_exists('tokio_worker_id') ? tokio_worker_id() : null;
?>
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Extension Test - tokio_php</title>
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
        .fail { color: #dc3545; }

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

    <h1>Extension Test</h1>
    <p class="subtitle">Test <code>tokio_sapi</code> PHP extension</p>

    <div class="section">
        <div class="section-title">Extension Status</div>
        <div class="card">
            <div class="row">
                <span class="label">tokio_sapi loaded</span>
                <span class="value <?= $extLoaded ? 'ok' : 'fail' ?>"><?= $extLoaded ? 'Yes' : 'No' ?></span>
            </div>
            <?php if (defined('TOKIO_SAPI_VERSION')): ?>
            <div class="row">
                <span class="label">Version</span>
                <span class="value"><?= TOKIO_SAPI_VERSION ?></span>
            </div>
            <?php endif; ?>
        </div>
    </div>

    <div class="section">
        <div class="section-title">Functions</div>
        <div class="card">
            <?php foreach ($functions as $fn => $exists): ?>
            <div class="row">
                <span class="label"><code><?= $fn ?>()</code></span>
                <span class="value <?= $exists ? 'ok' : 'fail' ?>"><?= $exists ? 'Available' : 'Not found' ?></span>
            </div>
            <?php endforeach; ?>
        </div>
    </div>

    <div class="section">
        <div class="section-title">Current Values</div>
        <div class="card">
            <div class="row">
                <span class="label">Request ID</span>
                <span class="value"><?= $requestId ?? 'N/A' ?></span>
            </div>
            <div class="row">
                <span class="label">Worker ID</span>
                <span class="value"><?= $workerId ?? 'N/A' ?></span>
            </div>
        </div>
    </div>

    <?php if ($serverInfo): ?>
    <div class="section">
        <div class="section-title">Server Info</div>
        <div class="card">
            <?php foreach ($serverInfo as $key => $val): ?>
            <div class="row">
                <span class="label"><?= htmlspecialchars($key) ?></span>
                <span class="value"><?= htmlspecialchars(is_array($val) ? json_encode($val) : $val) ?></span>
            </div>
            <?php endforeach; ?>
        </div>
    </div>
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
</body>
</html>
