<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>tokio_php - Rust + PHP</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            max-width: 800px;
            margin: 50px auto;
            padding: 20px;
            background: #1a1a2e;
            color: #eee;
        }
        h1 { color: #00d9ff; }
        .info { background: #16213e; padding: 20px; border-radius: 8px; margin: 20px 0; }
        .info h2 { color: #e94560; margin-top: 0; }
        code { background: #0f0f23; padding: 2px 6px; border-radius: 4px; }
        a { color: #00d9ff; }
    </style>
</head>
<body>
    <h1>🚀 tokio_php</h1>
    <p>Async Rust web server running PHP via php-embed</p>

    <div class="info">
        <h2>PHP Info</h2>
        <p><strong>PHP Version:</strong> <?= PHP_VERSION ?></p>
        <p><strong>Server Time:</strong> <?= date('Y-m-d H:i:s') ?></p>
        <p><strong>Server Software:</strong> tokio_php (Rust + Tokio + Hyper)</p>
        <p><strong>SAPI:</strong> <?= php_sapi_name() ?></p>
    </div>

    <div class="info">
        <h2>System Info</h2>
        <p><strong>OS:</strong> <?= php_uname() ?></p>
        <p><strong>Memory Usage:</strong> <?= number_format(memory_get_usage(true) / 1024 / 1024, 2) ?> MB</p>
    </div>

    <div class="info">
        <h2>Available Pages</h2>
        <ul>
            <li><a href="/info.php">PHP Info</a></li>
            <li><a href="/hello.php">Hello Example</a></li>
        </ul>
    </div>
</body>
</html>
