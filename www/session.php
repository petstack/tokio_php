<?php
/**
 * Custom session implementation for php-embed
 * PHP's native sessions don't work in php-embed because headers are considered "already sent"
 */
if (!class_exists('EmbedSession')) {
class EmbedSession {
    private static $data = [];
    private static $id = null;
    private static $save_path = '/tmp';

    public static function start() {
        // Get or generate session ID
        if (isset($_COOKIE['PHPSESSID']) && preg_match('/^[a-zA-Z0-9,-]{22,256}$/', $_COOKIE['PHPSESSID'])) {
            self::$id = $_COOKIE['PHPSESSID'];
        } else {
            self::$id = bin2hex(random_bytes(16));
            // Signal that we need to set a cookie
            header('Set-Cookie: PHPSESSID=' . self::$id . '; Path=/; HttpOnly; SameSite=Lax');
        }

        // Load session data
        $file = self::$save_path . '/sess_' . self::$id;
        if (file_exists($file)) {
            $content = file_get_contents($file);
            if ($content !== false) {
                self::$data = unserialize($content) ?: [];
            }
        }
    }

    public static function get($key, $default = null) {
        return self::$data[$key] ?? $default;
    }

    public static function set($key, $value) {
        self::$data[$key] = $value;
    }

    public static function delete($key) {
        unset(self::$data[$key]);
    }

    public static function clear() {
        self::$data = [];
    }

    public static function id() {
        return self::$id;
    }

    public static function all() {
        return self::$data;
    }

    public static function save() {
        if (self::$id) {
            $file = self::$save_path . '/sess_' . self::$id;
            file_put_contents($file, serialize(self::$data));
        }
    }

    public static function destroy() {
        $file = self::$save_path . '/sess_' . self::$id;
        if (file_exists($file)) {
            unlink($file);
        }
        self::$data = [];
        self::$id = null;
    }

    public static function reset() {
        self::$data = [];
        self::$id = null;
    }
}
} // end class_exists check

// Reset for new request
EmbedSession::reset();

// Start custom session
EmbedSession::start();

// Initialize visit counter
$visits = EmbedSession::get('visits', 0);
$visits++;
EmbedSession::set('visits', $visits);

// Handle actions
if (isset($_GET['action'])) {
    switch ($_GET['action']) {
        case 'set':
            $key = $_GET['key'] ?? 'test';
            $value = $_GET['value'] ?? 'value_' . time();
            EmbedSession::set($key, $value);
            break;
        case 'clear':
            EmbedSession::destroy();
            EmbedSession::set('visits', 1);
            $visits = 1;
            break;
    }
}

// Store last visit time
EmbedSession::set('last_visit', date('Y-m-d H:i:s'));

// Save session
EmbedSession::save();

// For compatibility, populate $_SESSION
$_SESSION = EmbedSession::all();
?>
<!DOCTYPE html>
<html>
<head>
    <title>Session Test - tokio_php</title>
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
        .counter { font-size: 48px; color: #00d9ff; text-align: center; margin: 20px 0; }
    </style>
</head>
<body>
    <h1>Session Test</h1>
    <p>Test <code>$_SESSION</code> support in tokio_php</p>

    <div class="counter">
        Visit #<?= $visits ?>
    </div>

    <div class="info">
        <h3>$_SESSION contents:</h3>
        <pre><?= htmlspecialchars(print_r($_SESSION, true)) ?></pre>
    </div>

    <div class="info">
        <h3>Session Info</h3>
        <p><strong>Session ID:</strong> <code><?= EmbedSession::id() ?></code></p>
        <p><strong>Session Handler:</strong> <code>EmbedSession (custom)</code></p>
        <p><strong>Save Path:</strong> <code>/tmp</code></p>
    </div>

    <div class="info">
        <h3>Actions</h3>
        <a class="btn btn-green" href="/session.php">Refresh (increment counter)</a>
        <a class="btn btn-green" href="/session.php?action=set&key=username&value=john_doe">Set username=john_doe</a>
        <a class="btn btn-green" href="/session.php?action=set&key=role&value=admin">Set role=admin</a>
        <a class="btn" href="/session.php?action=clear">Clear Session</a>
    </div>

    <div class="info">
        <h3>Test with curl</h3>
        <p>First request (get session cookie):</p>
        <pre>curl -c cookies.txt -b cookies.txt http://localhost:8080/session.php</pre>
        <p>Subsequent requests (use saved cookie):</p>
        <pre>curl -c cookies.txt -b cookies.txt http://localhost:8080/session.php</pre>
    </div>

    <p><a href="/">← Back to home</a></p>
</body>
</html>
