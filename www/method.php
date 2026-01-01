<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>HTTP Methods - tokio_php</title>
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
        .card { background: #fafafa; border: 1px solid #e8e8e8; border-radius: 8px; padding: 16px; margin-bottom: 16px; }
        .nav { margin-bottom: 24px; padding-bottom: 16px; border-bottom: 1px solid #eee; }

        .methods { display: flex; gap: 6px; flex-wrap: wrap; margin-bottom: 16px; }
        .method-btn { padding: 8px 14px; background: #f5f5f5; border: 1px solid #e8e8e8; border-radius: 6px; font-family: 'SF Mono', Consolas, monospace; font-size: 13px; font-weight: 600; color: #666; cursor: pointer; transition: all 0.15s ease; }
        .method-btn:hover { background: #eee; color: #333; }
        .method-btn.active { color: #fff; }
        .method-btn[data-method="GET"].active { background: #28a745; border-color: #28a745; }
        .method-btn[data-method="POST"].active { background: #ffc107; border-color: #ffc107; color: #333; }
        .method-btn[data-method="PUT"].active { background: #007bff; border-color: #007bff; }
        .method-btn[data-method="PATCH"].active { background: #17a2b8; border-color: #17a2b8; }
        .method-btn[data-method="DELETE"].active { background: #dc3545; border-color: #dc3545; }
        .method-btn[data-method="OPTIONS"].active { background: #6c757d; border-color: #6c757d; }
        .method-btn[data-method="QUERY"].active { background: #6f42c1; border-color: #6f42c1; }

        .form-group { margin-bottom: 12px; }
        label { display: block; font-size: 13px; color: #666; margin-bottom: 4px; }
        input[type="text"] { width: 100%; padding: 10px 12px; border: 1px solid #ddd; border-radius: 6px; font-size: 14px; font-family: 'SF Mono', Consolas, monospace; }
        input:focus { outline: none; border-color: #0066cc; }
        textarea { width: 100%; padding: 10px 12px; border: 1px solid #ddd; border-radius: 6px; font-size: 13px; font-family: 'SF Mono', Consolas, monospace; resize: vertical; min-height: 80px; }
        textarea:focus { outline: none; border-color: #0066cc; }
        button { padding: 10px 20px; background: #0066cc; color: #fff; border: none; border-radius: 6px; font-size: 14px; cursor: pointer; }
        button:hover { background: #0052a3; }
        button:disabled { background: #ccc; cursor: not-allowed; }

        .result { background: #1e1e1e; border-radius: 8px; overflow: hidden; margin-top: 16px; }
        .result-header { padding: 10px 14px; background: #2d2d2d; border-bottom: 1px solid #404040; display: flex; justify-content: space-between; align-items: center; }
        .result-label { font-size: 12px; color: #888; text-transform: uppercase; letter-spacing: 0.5px; }
        .result-status { font-family: 'SF Mono', Consolas, monospace; font-size: 13px; font-weight: 600; }
        .status-2xx { color: #4ec9b0; }
        .status-4xx { color: #ce9178; }
        .status-5xx { color: #f44747; }
        .result-body { padding: 14px; max-height: 300px; overflow-y: auto; }
        .result-body pre { margin: 0; font-family: 'SF Mono', Consolas, monospace; font-size: 13px; color: #d4d4d4; white-space: pre-wrap; word-break: break-all; }
        .result-empty { padding: 24px; text-align: center; color: #666; font-size: 13px; }

        .timing { font-size: 12px; color: #888; margin-top: 8px; text-align: right; }

        .examples { margin-top: 24px; }
        .example-btn { padding: 6px 10px; background: #f0f0f0; border: 1px solid #ddd; border-radius: 4px; font-size: 12px; color: #666; cursor: pointer; margin-right: 6px; margin-bottom: 6px; }
        .example-btn:hover { background: #e8e8e8; color: #333; }
    </style>
</head>
<body>
    <div class="nav"><a href="/">Home</a></div>

    <h1>HTTP Methods</h1>
    <p class="subtitle">Test <code>api.php</code> with different HTTP methods via AJAX</p>

    <div class="section">
        <div class="section-title">Method</div>
        <div class="methods">
            <button class="method-btn active" data-method="GET">GET</button>
            <button class="method-btn" data-method="POST">POST</button>
            <button class="method-btn" data-method="PUT">PUT</button>
            <button class="method-btn" data-method="PATCH">PATCH</button>
            <button class="method-btn" data-method="DELETE">DELETE</button>
            <button class="method-btn" data-method="OPTIONS">OPTIONS</button>
            <button class="method-btn" data-method="QUERY">QUERY</button>
        </div>
    </div>

    <div class="section">
        <div class="section-title">Request</div>
        <div class="card">
            <div class="form-group">
                <label for="url">URL</label>
                <input type="text" id="url" value="/api.php?id=123">
            </div>
            <div class="form-group" id="body-group">
                <label for="body">Body (JSON)</label>
                <textarea id="body" placeholder='{"key": "value"}'></textarea>
            </div>
            <button id="send-btn">Send Request</button>
        </div>

        <div class="examples">
            <span style="font-size: 12px; color: #999; margin-right: 8px;">Examples:</span>
            <button class="example-btn" data-method="GET" data-url="/api.php?id=42" data-body="">GET item</button>
            <button class="example-btn" data-method="POST" data-url="/api.php" data-body='{"name":"John","email":"john@example.com"}'>POST create</button>
            <button class="example-btn" data-method="PUT" data-url="/api.php?id=1" data-body='{"name":"Updated","status":"active"}'>PUT replace</button>
            <button class="example-btn" data-method="PATCH" data-url="/api.php?id=1" data-body='{"status":"inactive"}'>PATCH update</button>
            <button class="example-btn" data-method="DELETE" data-url="/api.php?id=99" data-body="">DELETE item</button>
            <button class="example-btn" data-method="QUERY" data-url="/api.php" data-body='{"search":"keyword","limit":10}'>QUERY search</button>
        </div>
    </div>

    <div class="section">
        <div class="section-title">Response</div>
        <div class="result" id="result">
            <div class="result-empty">Send a request to see the response</div>
        </div>
        <div class="timing" id="timing"></div>
    </div>

    <script>
        const methodBtns = document.querySelectorAll('.method-btn');
        const urlInput = document.getElementById('url');
        const bodyInput = document.getElementById('body');
        const bodyGroup = document.getElementById('body-group');
        const sendBtn = document.getElementById('send-btn');
        const resultDiv = document.getElementById('result');
        const timingDiv = document.getElementById('timing');
        const exampleBtns = document.querySelectorAll('.example-btn');

        let currentMethod = 'GET';

        // Method buttons
        methodBtns.forEach(btn => {
            btn.addEventListener('click', () => {
                methodBtns.forEach(b => b.classList.remove('active'));
                btn.classList.add('active');
                currentMethod = btn.dataset.method;
                updateBodyVisibility();
            });
        });

        // Example buttons
        exampleBtns.forEach(btn => {
            btn.addEventListener('click', () => {
                const method = btn.dataset.method;
                methodBtns.forEach(b => {
                    b.classList.toggle('active', b.dataset.method === method);
                });
                currentMethod = method;
                urlInput.value = btn.dataset.url;
                bodyInput.value = btn.dataset.body;
                updateBodyVisibility();
            });
        });

        function updateBodyVisibility() {
            const noBody = ['GET', 'HEAD'].includes(currentMethod);
            bodyGroup.style.display = noBody ? 'none' : 'block';
        }

        // Send request
        sendBtn.addEventListener('click', async () => {
            sendBtn.disabled = true;
            sendBtn.textContent = 'Sending...';
            timingDiv.textContent = '';

            const startTime = performance.now();

            try {
                const options = {
                    method: currentMethod,
                    headers: {}
                };

                if (!['GET', 'HEAD'].includes(currentMethod) && bodyInput.value.trim()) {
                    options.headers['Content-Type'] = 'application/json';
                    options.body = bodyInput.value.trim();
                }

                const response = await fetch(urlInput.value, options);
                const endTime = performance.now();
                const duration = (endTime - startTime).toFixed(2);

                const text = await response.text();
                let formatted = text;
                try {
                    const json = JSON.parse(text);
                    formatted = JSON.stringify(json, null, 2);
                } catch (e) {}

                const statusClass = response.status < 300 ? 'status-2xx' :
                                   response.status < 500 ? 'status-4xx' : 'status-5xx';

                resultDiv.innerHTML = `
                    <div class="result-header">
                        <span class="result-label">${currentMethod} ${urlInput.value}</span>
                        <span class="result-status ${statusClass}">${response.status} ${response.statusText}</span>
                    </div>
                    <div class="result-body"><pre>${escapeHtml(formatted)}</pre></div>
                `;

                timingDiv.textContent = `${duration} ms`;

            } catch (error) {
                resultDiv.innerHTML = `
                    <div class="result-header">
                        <span class="result-label">Error</span>
                        <span class="result-status status-5xx">Failed</span>
                    </div>
                    <div class="result-body"><pre>${escapeHtml(error.message)}</pre></div>
                `;
            }

            sendBtn.disabled = false;
            sendBtn.textContent = 'Send Request';
        });

        function escapeHtml(text) {
            const div = document.createElement('div');
            div.textContent = text;
            return div.innerHTML;
        }

        // Initialize
        updateBodyVisibility();
    </script>
<div style="margin-top: 32px; padding-top: 16px; border-top: 1px solid #eee; font-size: 12px; color: #999;"><?= number_format((microtime(true) - $_SERVER['REQUEST_TIME_FLOAT']) * 1000, 2) ?> ms</div>
</body>
</html>
