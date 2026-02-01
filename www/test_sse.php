<?php
/**
 * SSE (Server-Sent Events) test script with graceful reconnect support
 *
 * Usage with curl:
 *   curl -N -H "Accept: text/event-stream" http://localhost:8080/test_sse.php
 *
 * Or in browser JavaScript:
 *   const source = new EventSource('/test_sse.php');
 *   source.onmessage = (e) => console.log(e.data);
 *   source.addEventListener('reconnect', (e) => {
 *       console.log('Server requested reconnect:', e.data);
 *       source.close();
 *       // Reconnect after retry delay
 *   });
 */

// Disable output buffering for real-time streaming
while (ob_get_level()) {
    ob_end_clean();
}

// SSE headers
header('Content-Type: text/event-stream');
header('Cache-Control: no-cache');
header('Connection: keep-alive');
header('X-Accel-Buffering: no');  // Disable nginx buffering

// Ignore user abort to allow sending reconnect event
ignore_user_abort(true);

$count = 0;
$max_events = 30;  // 30 events, 1 per second = 30 seconds max

while ($count < $max_events) {
    // Check if client disconnected
    if (connection_aborted()) {
        break;
    }

    $count++;
    $data = json_encode([
        'event' => $count,
        'time' => date('H:i:s'),
        'message' => "Event $count of $max_events"
    ]);

    // SSE format: "data: {json}\n\n"
    echo "data: $data\n\n";
    flush();

    // Wait before next event
    if ($count < $max_events) {
        sleep(1);
    }
}

// Send reconnect event if client still connected
// This advises client to reconnect (e.g., to another server instance)
if (!connection_aborted()) {
    // Set retry interval (milliseconds) - client should wait this long before reconnecting
    echo "retry: 1000\n";
    echo "event: reconnect\n";
    echo "data: " . json_encode([
        'reason' => $count >= $max_events ? 'max_events_reached' : 'server_closing',
        'reconnect_after' => 1000
    ]) . "\n\n";
    flush();
}

// Send completion event
echo "data: " . json_encode(['event' => 'complete', 'total' => $count]) . "\n\n";
flush();
