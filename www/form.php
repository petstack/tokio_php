<?php
$message = '';
if ($_SERVER['REQUEST_METHOD'] === 'POST') {
    $name = htmlspecialchars($_POST['name'] ?? '', ENT_QUOTES, 'UTF-8');
    $email = htmlspecialchars($_POST['email'] ?? '', ENT_QUOTES, 'UTF-8');
    $message = "Received POST data: Name = '$name', Email = '$email'";
}
?>
<!DOCTYPE html>
<html>
<head>
    <title>POST Form Test - tokio_php</title>
    <style>
        body { font-family: sans-serif; margin: 50px auto; max-width: 600px; background: #1a1a2e; color: #eee; padding: 20px; }
        h1 { color: #00d9ff; }
        .form-group { margin: 15px 0; }
        label { display: block; margin-bottom: 5px; color: #8892bf; }
        input[type="text"], input[type="email"] {
            width: 100%; padding: 10px; border: 1px solid #4a4a6a;
            background: #16213e; color: #eee; border-radius: 4px;
        }
        button {
            background: #e94560; color: white; padding: 10px 20px;
            border: none; border-radius: 4px; cursor: pointer; font-size: 16px;
        }
        button:hover { background: #c73e54; }
        .result { background: #16213e; padding: 15px; border-radius: 8px; margin-top: 20px; }
        .info { background: #0f0f23; padding: 15px; border-radius: 8px; margin-top: 20px; }
        a { color: #e94560; }
        pre { white-space: pre-wrap; word-wrap: break-word; }
    </style>
</head>
<body>
    <h1>POST Form Test</h1>

    <?php if ($message): ?>
    <div class="result">
        <strong>✓ <?= $message ?></strong>
    </div>
    <?php endif; ?>

    <form method="POST" action="/form.php">
        <div class="form-group">
            <label for="name">Name:</label>
            <input type="text" id="name" name="name" placeholder="Enter your name">
        </div>
        <div class="form-group">
            <label for="email">Email:</label>
            <input type="email" id="email" name="email" placeholder="Enter your email">
        </div>
        <button type="submit">Submit (POST)</button>
    </form>

    <div class="info">
        <h3>$_POST contents:</h3>
        <pre><?= htmlspecialchars(print_r($_POST, true)) ?></pre>

        <h3>$_REQUEST contents:</h3>
        <pre><?= htmlspecialchars(print_r($_REQUEST, true)) ?></pre>

        <h3>Request Method:</h3>
        <pre><?= htmlspecialchars($_SERVER['REQUEST_METHOD'] ?? 'N/A') ?></pre>
    </div>

    <p><a href="/">← Back to home</a> | <a href="/hello.php?name=Test">Test GET</a></p>
</body>
</html>
