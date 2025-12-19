<?php
$name = $_GET['name'] ?? 'Guest';
$name = htmlspecialchars($name, ENT_QUOTES, 'UTF-8');
?>
<!DOCTYPE html>
<html>
<head>
    <title>Hello</title>
    <style>
        body { font-family: sans-serif; text-align: center; margin-top: 100px; background: #1a1a2e; color: #eee; }
        h1 { color: #00d9ff; font-size: 3em; }
    </style>
</head>
<body>
    <h1>Hello, <?= $name ?>!</h1>
    <p>Current time: <?= date('H:i:s') ?></p>
    <p><a href="/" style="color: #e94560;">← Back to home</a></p>
</body>
</html>
