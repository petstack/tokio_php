<?php
/**
 * SSE (Server-Sent Events) test script
 *
 * Usage with curl:
 *   curl -N -H "Accept: text/event-stream" http://localhost:8080/test_sse.php
 *
 * Or in browser JavaScript:
 *   const source = new EventSource('/test_sse.php');
 *   source.onmessage = (e) => console.log(e.data);
 */

// Note: Headers are set automatically by the server for SSE requests
header('Content-Type: text/event-stream');
$count = 0;
$max_events = 10;

while ($count < $max_events) {
    $count++;
    $data = json_encode([
        'event' => $count,
        'time' => date('H:i:s'),
        'message' => "Event $count of $max_events"
    ]);

    // SSE format: "data: {json}\n\n"
    echo "data: $data\n\n";

    // Flush the output to send to client immediately
    // Standard PHP flush() works via SAPI flush handler
    flush();

    // Wait before next event
    if ($count < $max_events) {
        sleep(1);
    }
}

// Send completion event
echo "data: " . json_encode(['event' => 'complete', 'total' => $max_events]) . "\n\n";
flush();
