<?php
// Set a test cookie if requested
if (isset($_GET['set'])) {
    $name = $_GET['set'];
    $value = $_GET['value'] ?? 'test_value_' . time();
    setcookie($name, $value, time() + 3600, '/');
    header('Location: /cookie.php');
    exit;
}

// Clear all cookies if requested
if (isset($_GET['clear'])) {
    foreach ($_COOKIE as $name => $value) {
        setcookie($name, '', time() - 3600, '/');
    }
    header('Location: /cookie.php');
    exit;
}
?>
<!DOCTYPE html>
<html>
<head>
    <title>Cookie Test - tokio_php</title>
    <style>
        body { font-family: sans-serif; margin: 50px auto; max-width: 600px; background: #1a1a2e; color: #eee; padding: 20px; }
        h1 { color: #00d9ff; }
        .info { background: #16213e; padding: 15px; border-radius: 8px; margin: 20px 0; }
        .info h3 { color: #e94560; margin-top: 0; }
        pre { background: #0f0f23; padding: 10px; border-radius: 4px; white-space: pre-wrap; word-wrap: break-word; }
        a { color: #00d9ff; }
        .btn { display: inline-block; background: #e94560; color: white; padding: 8px 16px; text-decoration: none; border-radius: 4px; margin: 5px; }
        .btn:hover { background: #c73e54; }
        .btn-green { background: #4caf50; }
        .btn-green:hover { background: #45a049; }
        code { background: #0f0f23; padding: 2px 6px; border-radius: 4px; }
    </style>
</head>
<body>
    <h1>Cookie Test</h1>
    <p>Test <code>$_COOKIE</code> support in tokio_php</p>

    <div class="info">
        <h3>$_COOKIE contents:</h3>
        <pre><?= htmlspecialchars(print_r($_COOKIE, true)) ?></pre>
        <p>Cookie count: <strong><?= count($_COOKIE) ?></strong></p>
    </div>

    <div class="info">
        <h3>Actions</h3>
        <a class="btn btn-green" href="/cookie.php?set=test_cookie&value=hello_world">Set Test Cookie</a>
        <a class="btn btn-green" href="/cookie.php?set=user_id&value=12345">Set user_id Cookie</a>
        <a class="btn" href="/cookie.php?clear=1">Clear All Cookies</a>
    </div>

    <div class="info">
        <h3>Manual Test</h3>
        <p>You can also test using curl:</p>
        <pre>curl -b "foo=bar; session=abc123" http://localhost:8080/cookie.php</pre>
    </div>

    <div class="info">
        <h3>Raw Cookie Header</h3>
        <pre><?= htmlspecialchars($_SERVER['HTTP_COOKIE'] ?? 'No Cookie header') ?></pre>
    </div>

    <p><a href="/">← Back to home</a></p>
</body>
</html>
