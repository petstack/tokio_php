# SSE Streaming

Server-Sent Events (SSE) support for real-time data streaming from PHP to clients.

## Overview

tokio_php supports SSE streaming with standard PHP `flush()` - no custom functions required. When a client requests with `Accept: text/event-stream`, the server automatically enables streaming mode.

## Quick Start

```php
<?php
// test_sse.php
while ($hasData) {
    $event = json_encode(['time' => date('H:i:s'), 'data' => getData()]);
    echo "data: $event\n\n";
    flush();  // Sends chunk to client immediately
    sleep(1);
}
```

```bash
# Test with curl
curl -N -H "Accept: text/event-stream" http://localhost:8080/test_sse.php

# Or in JavaScript
const source = new EventSource('/test_sse.php');
source.onmessage = (e) => console.log(JSON.parse(e.data));
```

## How It Works

### Request Flow

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

### Components

1. **SSE Detection** (`server/connection.rs`)
   - Checks `Accept: text/event-stream` header
   - Routes to `handle_sse_request()` handler

2. **Streaming Channel** (`bridge.rs`)
   - `StreamingChannel` wraps `mpsc::channel<StreamChunk>`
   - Callback sends chunks from PHP worker to async response

3. **Bridge TLS Context** (`ext/bridge/bridge.c`)
   - `tokio_bridge_enable_streaming()` - enables streaming mode
   - `tokio_bridge_send_chunk()` - sends data via callback
   - `tokio_bridge_end_stream()` - cleanup on completion

4. **SAPI Flush Handler** (`ext/tokio_sapi.c`)
   - Registered as `sapi_module.flush`
   - Intercepts PHP `flush()` calls
   - Flushes output buffers → reads from memfd → sends chunk

5. **Streaming Response** (`server/response/streaming.rs`)
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

For streams that run longer than `REQUEST_TIMEOUT`, use heartbeat:

```php
<?php
while ($streaming) {
    echo "data: " . json_encode($data) . "\n\n";
    flush();

    // Extend timeout by 30 seconds
    tokio_request_heartbeat(30);

    sleep(1);
}
```

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

Ensure you call `flush()` after each message:

```php
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
