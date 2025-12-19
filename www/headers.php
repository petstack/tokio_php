<?php
// Handle different header tests
$action = $_GET['action'] ?? '';

switch ($action) {
    case 'redirect':
        header('Location: /headers.php?action=redirected');
        exit;

    case 'redirect301':
        http_response_code(301);
        header('Location: /headers.php?action=redirected');
        exit;

    case 'redirected':
        $message = 'You were redirected here!';
        break;

    case 'json':
        header('Content-Type: application/json');
        echo json_encode([
            'status' => 'ok',
            'message' => 'This is JSON response',
            'time' => date('Y-m-d H:i:s')
        ]);
        exit;

    case 'custom':
        header('X-Custom-Header: Hello from PHP');
        header('X-Powered-By: tokio_php');
        header('X-Request-Time: ' . date('c'));
        break;

    case '404':
        http_response_code(404);
        break;

    case '500':
        http_response_code(500);
        break;

    case 'nocontent':
        http_response_code(204);
        exit;

    case 'earlyhints':
        // HTTP 103 Early Hints - send preload hints before main response
        http_response_code(103);
        header('Link: </style.css>; rel=preload; as=style');
        header('Link: </script.js>; rel=preload; as=script', false);
        // Note: In a real scenario, you would flush this and then send 200 OK
        // For testing, we just check if 103 status code works
        exit;

    default:
        $message = '';
}
?>
<!DOCTYPE html>
<html>
<head>
    <title>Headers Test - tokio_php</title>
    <style>
        body { font-family: sans-serif; margin: 50px auto; max-width: 700px; background: #1a1a2e; color: #eee; padding: 20px; }
        h1 { color: #00d9ff; }
        .info { background: #16213e; padding: 15px; border-radius: 8px; margin: 20px 0; }
        .info h3 { color: #e94560; margin-top: 0; }
        pre { background: #0f0f23; padding: 10px; border-radius: 4px; }
        a { color: #00d9ff; }
        .btn { display: inline-block; background: #e94560; color: white; padding: 8px 16px; text-decoration: none; border-radius: 4px; margin: 5px; }
        .btn:hover { background: #c73e54; }
        code { background: #0f0f23; padding: 2px 6px; border-radius: 4px; }
        .success { background: #2d5a3d; padding: 15px; border-radius: 8px; margin: 20px 0; }
    </style>
</head>
<body>
    <h1>Headers Test</h1>
    <p>Test PHP <code>header()</code> function support</p>

    <?php if (!empty($message)): ?>
    <div class="success">
        <strong><?= htmlspecialchars($message) ?></strong>
    </div>
    <?php endif; ?>

    <div class="info">
        <h3>Redirect Tests</h3>
        <a class="btn" href="/headers.php?action=redirect">302 Redirect</a>
        <a class="btn" href="/headers.php?action=redirect301">301 Redirect</a>
    </div>

    <div class="info">
        <h3>Content-Type Tests</h3>
        <a class="btn" href="/headers.php?action=json">JSON Response</a>
    </div>

    <div class="info">
        <h3>Status Code Tests</h3>
        <a class="btn" href="/headers.php?action=404">404 Not Found</a>
        <a class="btn" href="/headers.php?action=500">500 Error</a>
        <a class="btn" href="/headers.php?action=nocontent">204 No Content</a>
    </div>

    <div class="info">
        <h3>Custom Headers</h3>
        <a class="btn" href="/headers.php?action=custom">Add Custom Headers</a>
        <p style="color: #8892bf;">Check browser DevTools Network tab to see headers</p>
    </div>

    <div class="info">
        <h3>Test with curl</h3>
        <pre>curl -v http://localhost:8080/headers.php?action=redirect</pre>
        <pre>curl -v http://localhost:8080/headers.php?action=json</pre>
        <pre>curl -v http://localhost:8080/headers.php?action=custom</pre>
    </div>

    <div class="info">
        <h3>Current Headers (from PHP)</h3>
        <pre><?= htmlspecialchars(print_r(headers_list(), true)) ?></pre>
    </div>

    <p><a href="/">← Back to home</a></p>
</body>
</html>
