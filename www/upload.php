<?php
$message = '';
$uploaded = null;

if ($_SERVER['REQUEST_METHOD'] === 'POST' && isset($_FILES['file'])) {
    if ($_FILES['file']['error'] === 0) {
        $uploaded = $_FILES['file'];
        if (file_exists($uploaded['tmp_name'])) {
            $content = file_get_contents($uploaded['tmp_name'], false, null, 0, 100);
            $uploaded['preview'] = preg_match('/[\x00-\x08\x0B\x0C\x0E-\x1F]/', $content)
                ? '[Binary file]'
                : htmlspecialchars($content) . (filesize($uploaded['tmp_name']) > 100 ? '...' : '');
        }
        $message = 'File uploaded successfully!';
    } else {
        $errors = [1 => 'File too large', 2 => 'File too large', 3 => 'Partial upload', 4 => 'No file', 6 => 'No temp folder', 7 => 'Write failed'];
        $message = 'Error: ' . ($errors[$_FILES['file']['error']] ?? 'Unknown');
    }
}
?>
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Upload - tokio_php</title>
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
        .card { background: #fafafa; border: 1px solid #e8e8e8; border-radius: 8px; padding: 16px; margin-bottom: 16px; }
        .nav { margin-bottom: 24px; padding-bottom: 16px; border-bottom: 1px solid #eee; }
        .success { background: #d4edda; border: 1px solid #c3e6cb; color: #155724; padding: 12px 16px; border-radius: 6px; margin-bottom: 20px; }
        .error { background: #f8d7da; border: 1px solid #f5c6cb; color: #721c24; padding: 12px 16px; border-radius: 6px; margin-bottom: 20px; }
        input[type="file"] { margin-bottom: 12px; }
        button { padding: 10px 20px; background: #0066cc; color: #fff; border: none; border-radius: 6px; font-size: 14px; cursor: pointer; }
        button:hover { background: #0052a3; }
        table { width: 100%; border-collapse: collapse; font-size: 14px; }
        td { padding: 8px 0; border-bottom: 1px solid #eee; }
        td:first-child { color: #666; width: 120px; }

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

    <h1>File Upload</h1>
    <p class="subtitle">Test <code>$_FILES</code> superglobal</p>

    <?php if ($message): ?>
    <div class="<?= strpos($message, 'Error') !== false ? 'error' : 'success' ?>"><?= htmlspecialchars($message) ?></div>
    <?php endif; ?>

    <?php if ($uploaded): ?>
    <div class="section">
        <div class="section-title">Uploaded File</div>
        <div class="card">
            <table>
                <tr><td>Name</td><td><code><?= htmlspecialchars($uploaded['name']) ?></code></td></tr>
                <tr><td>Type</td><td><code><?= htmlspecialchars($uploaded['type']) ?></code></td></tr>
                <tr><td>Size</td><td><?= number_format($uploaded['size']) ?> bytes</td></tr>
                <tr><td>Temp</td><td><code><?= htmlspecialchars($uploaded['tmp_name']) ?></code></td></tr>
            </table>
            <?php if (!empty($uploaded['preview'])): ?>
            <div class="section-title" style="margin-top: 16px;">Preview</div>
            <pre><?= $uploaded['preview'] ?></pre>
            <?php endif; ?>
        </div>
    </div>
    <?php endif; ?>

    <div class="section">
        <div class="section-title">Upload Form</div>
        <div class="card">
            <form method="POST" enctype="multipart/form-data">
                <input type="file" name="file">
                <button type="submit">Upload</button>
            </form>
        </div>
    </div>

    <div class="section">
        <div class="section-title">Test with curl</div>
        <pre>curl -F "file=@/path/to/file.txt" http://localhost:8080/upload.php</pre>
    </div>

    <div class="section">
        <div class="section-title">Superglobals</div>
        <div class="tabs">
            <div class="tab" data-tab="get">$_GET</div>
            <div class="tab" data-tab="post">$_POST</div>
            <div class="tab" data-tab="server">$_SERVER</div>
            <div class="tab" data-tab="cookie">$_COOKIE</div>
            <div class="tab active" data-tab="files">$_FILES</div>
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
