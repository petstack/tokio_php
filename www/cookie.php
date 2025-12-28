<?php
if (isset($_GET['set'])) {
    $name = $_GET['set'];
    $value = $_GET['value'] ?? 'test_value_' . time();
    setcookie($name, $value, time() + 3600, '/');
    header('Location: /cookie.php');
    exit;
}
if (isset($_GET['clear'])) {
    foreach ($_COOKIE as $name => $value) {
        setcookie($name, '', time() - 3600, '/');
    }
    header('Location: /cookie.php');
    exit;
}
?>
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Cookies - tokio_php</title>
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
        pre { background: #f5f5f5; padding: 12px; border-radius: 6px; font-size: 13px; overflow-x: auto; font-family: 'SF Mono', Consolas, monospace; }
        .btn { display: inline-block; padding: 8px 16px; background: #0066cc; color: #fff; border-radius: 6px; font-size: 14px; margin-right: 8px; margin-bottom: 8px; }
        .btn:hover { background: #0052a3; text-decoration: none; }
        .btn-danger { background: #dc3545; }
        .btn-danger:hover { background: #c82333; }
        .card { background: #fafafa; border: 1px solid #e8e8e8; border-radius: 8px; padding: 16px; margin-bottom: 16px; }
        .nav { margin-bottom: 24px; padding-bottom: 16px; border-bottom: 1px solid #eee; }

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

    <h1>Cookies</h1>
    <p class="subtitle">Test <code>$_COOKIE</code> superglobal</p>

    <div class="section">
        <div class="section-title">Actions</div>
        <a class="btn" href="/cookie.php?set=test_cookie&value=hello_world">Set test_cookie</a>
        <a class="btn" href="/cookie.php?set=user_id&value=12345">Set user_id</a>
        <a class="btn btn-danger" href="/cookie.php?clear=1">Clear All</a>
    </div>

    <div class="section">
        <div class="section-title">Current Cookies (<?= count($_COOKIE) ?>)</div>
        <div class="card">
            <?php if (empty($_COOKIE)): ?>
                <p style="color: #999;">No cookies set</p>
            <?php else: ?>
                <pre><?= htmlspecialchars(print_r($_COOKIE, true)) ?></pre>
            <?php endif; ?>
        </div>
    </div>

    <div class="section">
        <div class="section-title">Raw Cookie Header</div>
        <div class="card">
            <code><?= htmlspecialchars($_SERVER['HTTP_COOKIE'] ?? 'None') ?></code>
        </div>
    </div>

    <div class="section">
        <div class="section-title">Test with curl</div>
        <pre>curl -b "foo=bar; session=abc123" http://localhost:8080/cookie.php</pre>
    </div>

    <div class="section">
        <div class="section-title">Superglobals</div>
        <div class="tabs">
            <div class="tab" data-tab="get">$_GET</div>
            <div class="tab" data-tab="post">$_POST</div>
            <div class="tab" data-tab="server">$_SERVER</div>
            <div class="tab active" data-tab="cookie">$_COOKIE</div>
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
