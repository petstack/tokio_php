# SSE Streaming

Server-Sent Events (SSE) support for real-time data streaming from PHP to clients.

## Overview

tokio_php supports SSE streaming with standard PHP `flush()` - no custom functions required. Streaming mode is enabled through two methods:

1. **Explicit (client-driven)**: Client sends `Accept: text/event-stream` header
2. **Auto-detect (server-driven)**: PHP sets `header('Content-Type: text/event-stream')`

Both methods work transparently with the same PHP code.

## Quick Start

```php
<?php
// test_sse.php
header('Content-Type: text/event-stream');  // Enables streaming (auto-detect)
header('Cache-Control: no-cache');
header('Connection: keep-alive');

while ($hasData) {
    $event = json_encode(['time' => date('H:i:s'), 'data' => getData()]);
    echo "data: $event\n\n";
    flush();  // Sends chunk to client immediately
    sleep(1);
}
```

```bash
# Test with curl (auto-detect via Content-Type)
curl -N http://localhost:8080/test_sse.php

# Or with explicit Accept header
curl -N -H "Accept: text/event-stream" http://localhost:8080/test_sse.php

# JavaScript EventSource (sends Accept header automatically)
const source = new EventSource('/test_sse.php');
source.onmessage = (e) => console.log(JSON.parse(e.data));
```

## How It Works

### Detection Methods

| Method | Trigger | Use Case |
|--------|---------|----------|
| **Explicit** | `Accept: text/event-stream` header | EventSource API, SSE clients |
| **Auto-detect** | `header('Content-Type: text/event-stream')` in PHP | Any client, curl, custom scripts |

**Explicit mode**: Server detects `Accept` header before PHP execution and pre-enables streaming.

**Auto-detect mode**: Server prepares a callback, then PHP's `header()` call triggers streaming when Content-Type matches.

### Request Flow (Explicit Mode)

```
Client                     Server                       PHP Worker
  │                          │                               │
  │  GET /sse.php            │                               │
  │  Accept: text/event-     │                               │
  │  stream                  │                               │
  │ ─────────────────────────►                               │
  │                          │                               │
  │                          │  1. Detect SSE request        │
  │                          │  2. Create streaming channel  │
  │                          │  3. Enable streaming in bridge│
  │                          │ ──────────────────────────────►
  │                          │                               │
  │  HTTP 200                │                               │
  │  Content-Type:           │                               │
  │  text/event-stream       │                               │
  │ ◄─────────────────────────                               │
  │                          │                               │
  │                          │     echo "data: ...\n\n";     │
  │                          │     flush();                  │
  │                          │                               │
  │                          │  4. SAPI flush handler:       │
  │                          │     - Flush PHP buffers       │
  │                          │     - Read new output         │
  │                          │     - Send via callback       │
  │                          │ ◄──────────────────────────────
  │                          │                               │
  │  data: ...               │                               │
  │ ◄─────────────────────────                               │
  │                          │                               │
  │      ... repeat for each flush() ...                     │
  │                          │                               │
  │                          │  5. Script ends               │
  │                          │     end_stream()              │
  │  (connection closes)     │ ◄──────────────────────────────
  │ ◄─────────────────────────                               │
```

### Request Flow (Auto-detect Mode)

```
Client                     Server                       PHP Worker
  │                          │                               │
  │  GET /sse.php            │                               │
  │  (no Accept header)      │                               │
  │ ─────────────────────────►                               │
  │                          │                               │
  │                          │  1. Normal request handling   │
  │                          │  2. Set stream callback       │
  │                          │     (not enabled yet)         │
  │                          │ ──────────────────────────────►
  │                          │                               │
  │                          │     header('Content-Type:     │
  │                          │     text/event-stream');      │
  │                          │                               │
  │                          │  3. SAPI header_handler:      │
  │                          │     - Detect Content-Type     │
  │                          │     - try_enable_streaming()  │
  │                          │     - Streaming NOW enabled   │
  │                          │ ◄──────────────────────────────
  │                          │                               │
  │  HTTP 200                │                               │
  │  Content-Type:           │                               │
  │  text/event-stream       │                               │
  │ ◄─────────────────────────                               │
  │                          │                               │
  │                          │     echo "data: ...\n\n";     │
  │                          │     flush();                  │
  │                          │                               │
  │                          │  4. SAPI flush handler:       │
  │                          │     - Send via callback       │
  │                          │ ◄──────────────────────────────
  │                          │                               │
  │  data: ...               │                               │
  │ ◄─────────────────────────                               │
  │                          │                               │
  │      ... repeat for each flush() ...                     │
  │                          │                               │
  │                          │  5. Script ends               │
  │                          │     end_stream()              │
  │  (connection closes)     │ ◄──────────────────────────────
  │ ◄─────────────────────────                               │
```

### Components

1. **SSE Detection** (`server/connection.rs`)
   - Checks `Accept: text/event-stream` header for explicit mode
   - Uses `execute_with_auto_sse()` for auto-detect mode
   - Returns `ExecuteResult::Normal` or `ExecuteResult::Streaming`

2. **Streaming Channel** (`bridge.rs`)
   - `StreamingChannel` wraps `mpsc::channel<StreamChunk>`
   - Callback sends chunks from PHP worker to async response

3. **Bridge TLS Context** (`ext/bridge/bridge.c`)
   - `tokio_bridge_enable_streaming()` - enables streaming immediately (explicit mode)
   - `tokio_bridge_set_stream_callback()` - sets callback without enabling (auto-detect)
   - `tokio_bridge_try_enable_streaming()` - enables if callback configured (called by SAPI)
   - `tokio_bridge_send_chunk()` - sends data via callback
   - `tokio_bridge_end_stream()` - cleanup on completion

4. **SAPI Header Handler** (`executor/sapi.rs`)
   - Registered as `sapi_module.header_handler`
   - Intercepts PHP `header()` calls
   - Detects `Content-Type: text/event-stream`
   - Calls `try_enable_streaming()` to activate streaming mode

5. **SAPI Flush Handler** (`executor/sapi.rs`)
   - Registered as `sapi_module.flush`
   - Intercepts PHP `flush()` calls
   - Flushes output buffers → sends chunk via bridge callback

6. **Streaming Response** (`server/response/streaming.rs`)
   - `ChunkFrameStream` converts channel to HTTP frames
   - `StreamBody` wraps stream for Hyper

## Response Headers

SSE responses automatically include:

| Header | Value | Purpose |
|--------|-------|---------|
| `Content-Type` | `text/event-stream` | SSE MIME type |
| `Cache-Control` | `no-cache` | Disable caching |
| `Connection` | `keep-alive` | Persistent connection |
| `X-Accel-Buffering` | `no` | Disable nginx buffering |

## SSE Format

Standard SSE format (RFC 8895):

```
data: {"event": 1, "message": "Hello"}\n\n
data: {"event": 2, "message": "World"}\n\n
```

Each message:
- Starts with `data: `
- Ends with `\n\n` (double newline)
- Can span multiple lines with `data:` prefix on each

### Named Events

```php
echo "event: update\n";
echo "data: {\"status\": \"active\"}\n\n";
flush();
```

```javascript
source.addEventListener('update', (e) => {
    console.log(JSON.parse(e.data));
});
```

### Event ID and Retry

```php
echo "id: 123\n";
echo "retry: 5000\n";  // Reconnect after 5 seconds
echo "data: message\n\n";
flush();
```

## Examples

### Basic Counter

```php
<?php
header('Content-Type: text/event-stream');
header('Cache-Control: no-cache');
header('Connection: keep-alive');

$count = 0;
while ($count < 10) {
    $count++;
    echo "data: " . json_encode(['count' => $count]) . "\n\n";
    flush();
    sleep(1);
}
```

### Real-time Notifications

```php
<?php
header('Content-Type: text/event-stream');
header('Cache-Control: no-cache');
header('Connection: keep-alive');

while (true) {
    $notifications = getNewNotifications();

    if (!empty($notifications)) {
        foreach ($notifications as $notification) {
            echo "event: notification\n";
            echo "data: " . json_encode($notification) . "\n\n";
        }
        flush();
    }

    // Check every 2 seconds
    sleep(2);

    // Extend timeout for long-running connections
    tokio_request_heartbeat(30);
}
```

### Progress Updates

```php
<?php
header('Content-Type: text/event-stream');
header('Cache-Control: no-cache');
header('Connection: keep-alive');

$total = count($items);
$processed = 0;

foreach ($items as $item) {
    processItem($item);
    $processed++;

    $progress = [
        'processed' => $processed,
        'total' => $total,
        'percent' => round($processed / $total * 100)
    ];

    echo "event: progress\n";
    echo "data: " . json_encode($progress) . "\n\n";
    flush();
}

echo "event: complete\n";
echo "data: {\"status\": \"done\"}\n\n";
flush();
```

### JavaScript Client

```html
<script>
const output = document.getElementById('output');
const source = new EventSource('/test_sse.php');

source.onopen = () => {
    console.log('Connected');
};

source.onmessage = (event) => {
    const data = JSON.parse(event.data);
    output.innerHTML += `<p>${data.message}</p>`;
};

source.addEventListener('progress', (event) => {
    const data = JSON.parse(event.data);
    document.getElementById('progress').value = data.percent;
});

source.onerror = (error) => {
    if (source.readyState === EventSource.CLOSED) {
        console.log('Connection closed');
    }
};

// Close connection when done
function stop() {
    source.close();
}
</script>
```

## Long-Running Streams

SSE connections use a dedicated `SSE_TIMEOUT` (default: 30 minutes) separate from `REQUEST_TIMEOUT`. For streams that need to run longer, use heartbeat:

```php
<?php
header('Content-Type: text/event-stream');
header('Cache-Control: no-cache');
header('Connection: keep-alive');

while ($streaming) {
    echo "data: " . json_encode($data) . "\n\n";
    flush();

    // Extend timeout by 30 seconds
    tokio_request_heartbeat(30);

    sleep(1);
}
```

### SSE_TIMEOUT Configuration

```bash
# Default: 30 minutes
SSE_TIMEOUT=30m

# 1 hour for long-running streams
SSE_TIMEOUT=1h

# Disable timeout (not recommended)
SSE_TIMEOUT=off
```

See [Configuration](configuration.md#sse_timeout) for more details.

## Compression

SSE responses are **not compressed** by default:
- `text/event-stream` is not in the compressible MIME types list
- Compression would buffer chunks, defeating real-time streaming

## Limitations

- **No bidirectional communication** - SSE is server-to-client only (use WebSockets for bidirectional)
- **Text only** - Binary data must be base64 encoded
- **Connection limits** - Browsers limit concurrent SSE connections per domain (typically 6)
- **No IE support** - Internet Explorer doesn't support EventSource (use polyfill)

## Troubleshooting

### Chunks arrive all at once

Ensure you set the Content-Type header and call `flush()` after each message:

```php
header('Content-Type: text/event-stream');  // Required for auto-detect!
echo "data: message\n\n";
flush();  // Required!
```

### Connection drops after timeout

Use `tokio_request_heartbeat()` for long-running streams:

```php
tokio_request_heartbeat(30);  // Extend by 30 seconds
```

### Nginx buffering

If behind nginx, add to your config:

```nginx
location /sse {
    proxy_buffering off;
    proxy_cache off;
    proxy_read_timeout 86400s;
}
```

The server already sends `X-Accel-Buffering: no` header.

## See Also

- [Request Heartbeat](request-heartbeat.md) - Timeout extension for long-running scripts
- [Architecture](architecture.md) - System overview
- [tokio_sapi Extension](tokio-sapi-extension.md) - PHP extension details
