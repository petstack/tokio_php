<?php
$name = $_GET['name'] ?? 'Guest';
$name = htmlspecialchars($name, ENT_QUOTES, 'UTF-8');
?>
<!DOCTYPE html>
<html>
<head>
    <title>Hello - tokio_php</title>
    <style>
        body { font-family: sans-serif; text-align: center; margin-top: 50px; background: #1a1a2e; color: #eee; }
        h1 { color: #00d9ff; font-size: 3em; }
        .info { background: #16213e; padding: 20px; border-radius: 8px; margin: 20px auto; max-width: 500px; text-align: left; }
        a { color: #e94560; }
        code { background: #0f0f23; padding: 2px 6px; border-radius: 4px; }
    </style>
</head>
<body>
    <h1>Hello, <?= $name ?>!</h1>
    <p>Current time: <?= date('H:i:s') ?></p>

    <div class="info">
        <h3>$_GET contents:</h3>
        <pre><?= htmlspecialchars(print_r($_GET, true)) ?></pre>
    </div>

    <div class="info">
        <h3>$_SERVER (selected):</h3>
        <pre><?php
        $keys = ['REQUEST_METHOD', 'REQUEST_URI', 'QUERY_STRING', 'REMOTE_ADDR', 'SERVER_SOFTWARE'];
        foreach ($keys as $key) {
            if (isset($_SERVER[$key])) {
                echo htmlspecialchars("$key: {$_SERVER[$key]}") . "\n";
            }
        }
        ?></pre>
    </div>

    <p>Try: <code>/hello.php?name=World&foo=bar</code></p>
    <p><a href="/">← Back to home</a> | <a href="/form.php">Test POST form</a></p>
</body>
</html>
