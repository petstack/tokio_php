<?php
/**
 * Long-running SSE test script with heartbeat
 *
 * Tests the SSE timeout extension via tokio_request_heartbeat()
 *
 * Usage:
 *   curl -N -H "Accept: text/event-stream" http://localhost:8080/test_sse_long.php
 *   curl -N -H "Accept: text/event-stream" "http://localhost:8080/test_sse_long.php?duration=30"
 */

$start = time();
$duration = isset($_GET['duration']) ? min((int)$_GET['duration'], 300) : 10; // Max 5 minutes

$count = 0;

while (time() - $start < $duration) {
    $count++;
    $elapsed = time() - $start;

    // Extend request timeout by 30 seconds
    tokio_request_heartbeat(30);

    // Also extend PHP's internal time limit
    set_time_limit(30);

    $data = json_encode([
        'event' => $count,
        'elapsed' => $elapsed,
        'remaining' => $duration - $elapsed,
        'memory' => memory_get_usage(true),
        'time' => date('H:i:s'),
    ]);

    echo "data: $data\n\n";
    flush();  // Standard PHP flush() works via SAPI flush handler

    sleep(1);
}

// Send completion event
echo "event: close\n";
echo "data: " . json_encode([
    'status' => 'finished',
    'total_events' => $count,
    'duration' => time() - $start,
]) . "\n\n";
flush();
