<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>tokio_php</title>
    <style>
        * { box-sizing: border-box; margin: 0; padding: 0; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            max-width: 720px;
            margin: 40px auto;
            padding: 20px;
            background: #fff;
            color: #333;
            line-height: 1.6;
        }
        h1 { font-size: 24px; font-weight: 600; margin-bottom: 8px; }
        .subtitle { color: #666; margin-bottom: 32px; }
        .section { margin-bottom: 28px; }
        .section-title { font-size: 12px; font-weight: 600; color: #999; text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 12px; }
        .row { display: flex; justify-content: space-between; padding: 8px 0; border-bottom: 1px solid #f0f0f0; }
        .row:last-child { border-bottom: none; }
        .label { color: #666; }
        .value { color: #333; font-weight: 500; }
        a { color: #0066cc; text-decoration: none; }
        a:hover { text-decoration: underline; }
        ul { list-style: none; }
        li { padding: 6px 0; border-bottom: 1px solid #f0f0f0; }
        li:last-child { border-bottom: none; }
        .desc { color: #999; font-size: 13px; margin-left: 8px; }

        /* Tabs */
        .tabs { display: flex; gap: 6px; flex-wrap: wrap; margin-bottom: 12px; }
        .tab {
            padding: 6px 12px;
            background: #f5f5f5;
            border: 1px solid #e8e8e8;
            border-radius: 6px;
            font-family: 'SF Mono', Consolas, monospace;
            font-size: 13px;
            color: #666;
            cursor: pointer;
            transition: all 0.15s ease;
        }
        .tab:hover { background: #eee; color: #333; }
        .tab.active { background: #0066cc; border-color: #0066cc; color: #fff; }

        /* Tab content */
        .tab-content {
            display: none;
            background: #fafafa;
            border: 1px solid #e8e8e8;
            border-radius: 8px;
            overflow: hidden;
        }
        .tab-content.active { display: block; }
        .tab-header {
            padding: 10px 14px;
            background: #f0f0f0;
            border-bottom: 1px solid #e8e8e8;
            font-size: 12px;
            color: #666;
        }
        .tab-body {
            max-height: 320px;
            overflow-y: auto;
        }
        .tab-empty {
            padding: 24px;
            text-align: center;
            color: #999;
            font-size: 13px;
        }

        /* Key-value table */
        .kv-table { width: 100%; border-collapse: collapse; font-size: 13px; }
        .kv-table tr { border-bottom: 1px solid #eee; }
        .kv-table tr:last-child { border-bottom: none; }
        .kv-table tr:hover { background: #f5f5f5; }
        .kv-table td { padding: 8px 14px; vertical-align: top; }
        .kv-table .key {
            width: 35%;
            font-family: 'SF Mono', Consolas, monospace;
            color: #0066cc;
            word-break: break-all;
        }
        .kv-table .val {
            color: #333;
            word-break: break-all;
            font-family: 'SF Mono', Consolas, monospace;
        }
        .kv-table .val-string { color: #22863a; }
        .kv-table .val-number { color: #005cc5; }
        .kv-table .val-array { color: #6f42c1; }
    </style>
</head>
<body>
    <h1>tokio_php</h1>
    <p class="subtitle">Async PHP server powered by Rust</p>

    <div class="section">
        <div class="section-title">Server</div>
        <div class="row"><span class="label">PHP</span><span class="value"><?= PHP_VERSION ?></span></div>
        <div class="row"><span class="label">SAPI</span><span class="value"><?= php_sapi_name() ?></span></div>
        <div class="row"><span class="label">Time</span><span class="value"><?= date('H:i:s') ?></span></div>
        <div class="row"><span class="label">Memory</span><span class="value"><?= number_format(memory_get_usage(true) / 1024 / 1024, 1) ?> MB</span></div>
    </div>

    <div class="section">
        <div class="section-title">Pages</div>
        <ul>
            <li><a href="/info.php">phpinfo()</a><span class="desc">Full PHP info</span></li>
            <li><a href="/hello.php?name=World">hello.php</a><span class="desc">GET params</span></li>
            <li><a href="/form.php">form.php</a><span class="desc">POST form</span></li>
            <li><a href="/cookie.php">cookie.php</a><span class="desc">Cookies</span></li>
            <li><a href="/upload.php">upload.php</a><span class="desc">File uploads</span></li>
            <li><a href="/headers.php">headers.php</a><span class="desc">Headers &amp; redirects</span></li>
            <li><a href="/opcache_status.php">opcache_status.php</a><span class="desc">OPcache stats</span></li>
            <li><a href="/ext_test.php">ext_test.php</a><span class="desc">Extension test</span></li>
            <li><a href="/method.php">method.php</a><span class="desc">HTTP methods tester</span></li>
            <li><a href="/api.php">api.php</a><span class="desc">REST API example</span></li>
            <li><a href="/test_sse.php">test_sse.php</a><span class="desc">SSE streaming</span></li>
        </ul>
    </div>

    <div class="section">
        <div class="section-title">SSE Demo</div>
        <div style="display: flex; gap: 12px; align-items: center; margin-bottom: 12px;">
            <button id="sse-start" style="padding: 8px 16px; background: #0066cc; color: #fff; border: none; border-radius: 6px; cursor: pointer; font-size: 14px;">Start SSE</button>
            <button id="sse-stop" style="padding: 8px 16px; background: #dc3545; color: #fff; border: none; border-radius: 6px; cursor: pointer; font-size: 14px;" disabled>Stop</button>
            <span id="sse-status" style="color: #999; font-size: 13px;">Not connected</span>
        </div>
        <div id="sse-output" style="background: #1e1e1e; color: #d4d4d4; padding: 14px; border-radius: 8px; font-family: 'SF Mono', Consolas, monospace; font-size: 13px; height: 200px; overflow-y: auto; white-space: pre-wrap;"></div>
    </div>

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

        <?php
        function renderTable(array $data, string $id, string $title): void {
            $count = count($data);
            echo "<div class=\"tab-content\" id=\"tab-{$id}\">";
            echo "<div class=\"tab-header\">{$title} ({$count} " . ($count === 1 ? 'item' : 'items') . ")</div>";
            echo "<div class=\"tab-body\">";
            if (empty($data)) {
                echo "<div class=\"tab-empty\">Empty</div>";
            } else {
                echo "<table class=\"kv-table\">";
                foreach ($data as $key => $value) {
                    $key = htmlspecialchars((string)$key);
                    if (is_array($value)) {
                        $displayValue = htmlspecialchars(json_encode($value, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES));
                        $class = 'val-array';
                    } elseif (is_numeric($value)) {
                        $displayValue = htmlspecialchars((string)$value);
                        $class = 'val-number';
                    } else {
                        $displayValue = htmlspecialchars((string)$value);
                        $class = 'val-string';
                    }
                    echo "<tr><td class=\"key\">{$key}</td><td class=\"val {$class}\">{$displayValue}</td></tr>";
                }
                echo "</table>";
            }
            echo "</div></div>";
        }

        renderTable($_GET, 'get', '$_GET');
        renderTable($_POST, 'post', '$_POST');
        renderTable($_SERVER, 'server', '$_SERVER');
        renderTable($_COOKIE, 'cookie', '$_COOKIE');
        renderTable($_FILES, 'files', '$_FILES');
        renderTable($_REQUEST, 'request', '$_REQUEST');
        ?>
    </div>

    <script>
        // Tabs
        document.querySelectorAll('.tab').forEach(tab => {
            tab.addEventListener('click', () => {
                document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
                document.querySelectorAll('.tab-content').forEach(c => c.classList.remove('active'));
                tab.classList.add('active');
                document.getElementById('tab-' + tab.dataset.tab).classList.add('active');
            });
        });
        document.querySelector('.tab-content').classList.add('active');

        // SSE Demo
        let eventSource = null;
        const output = document.getElementById('sse-output');
        const status = document.getElementById('sse-status');
        const startBtn = document.getElementById('sse-start');
        const stopBtn = document.getElementById('sse-stop');

        function log(msg, color = '#d4d4d4') {
            const time = new Date().toLocaleTimeString();
            output.innerHTML += `<span style="color:#6a9955">[${time}]</span> <span style="color:${color}">${msg}</span>\n`;
            output.scrollTop = output.scrollHeight;
        }

        startBtn.addEventListener('click', () => {
            output.innerHTML = '';
            log('Connecting to /test_sse.php...', '#569cd6');

            eventSource = new EventSource('/test_sse.php');

            eventSource.onopen = () => {
                status.textContent = 'Connected';
                status.style.color = '#28a745';
                startBtn.disabled = true;
                stopBtn.disabled = false;
                log('Connection established', '#4ec9b0');
            };

            eventSource.onmessage = (e) => {
                try {
                    const data = JSON.parse(e.data);
                    log(`Event ${data.event}: ${data.message}`, '#ce9178');
                } catch {
                    log(e.data, '#ce9178');
                }
            };

            eventSource.onerror = () => {
                if (eventSource.readyState === EventSource.CLOSED) {
                    log('Connection closed', '#569cd6');
                    status.textContent = 'Disconnected';
                    status.style.color = '#999';
                    startBtn.disabled = false;
                    stopBtn.disabled = true;
                    eventSource = null;
                }
            };
        });

        stopBtn.addEventListener('click', () => {
            if (eventSource) {
                eventSource.close();
                log('Connection closed by user', '#dcdcaa');
                status.textContent = 'Disconnected';
                status.style.color = '#999';
                startBtn.disabled = false;
                stopBtn.disabled = true;
                eventSource = null;
            }
        });
    </script>
<div style="margin-top: 32px; padding-top: 16px; border-top: 1px solid #eee; font-size: 12px; color: #999;"><?= number_format((microtime(true) - $_SERVER['REQUEST_TIME_FLOAT']) * 1000, 2) ?> ms</div>
</body>
</html>
