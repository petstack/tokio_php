<?php
/**
 * SSE (Server-Sent Events) test script with configurable timing
 *
 * Parameters:
 *   count - Number of events to send (default: 5)
 *   delay - Delay between events in milliseconds (default: 1000)
 *
 * Usage:
 *   curl -N http://localhost:8080/test_sse_timed.php?count=3&delay=500
 */

header('Content-Type: text/event-stream');
header('Cache-Control: no-cache');
header('Connection: keep-alive');

// Get parameters
$count = max(1, min(100, (int)($_GET['count'] ?? 5)));
$delay_ms = max(0, min(10000, (int)($_GET['delay'] ?? 1000)));

for ($i = 1; $i <= $count; $i++) {
    $data = json_encode([
        'event' => $i,
        'time' => date('H:i:s.') . substr(microtime(), 2, 3),
        'message' => "Event $i of $count",
        'delay_ms' => $delay_ms,
    ]);

    echo "data: $data\n\n";
    flush();

    // Delay before next event (except for last one)
    if ($i < $count && $delay_ms > 0) {
        usleep($delay_ms * 1000);
    }
}
