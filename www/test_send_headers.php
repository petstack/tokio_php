<?php
/**
 * Test script for send_headers SAPI callback.
 *
 * Tests that HTTP headers are sent before the first body output,
 * which is important for SSE and other streaming scenarios.
 */

$test_type = $_GET['test'] ?? 'info';

switch ($test_type) {
    case 'headers_timing':
        // Test that headers are sent before body
        // Record time before setting headers
        $start = microtime(true);

        header('Content-Type: application/json');
        header('X-Custom-Header: test-value');
        header('X-Headers-Set-At: ' . $start);

        // Check if headers_sent() returns true after header() calls
        // Note: headers_sent() may still return false until actual output
        $headers_sent_before_output = headers_sent();

        // First output - this should trigger send_headers if not already sent
        $first_output_time = microtime(true);

        echo json_encode([
            'status' => 'ok',
            'test' => 'headers_timing',
            'start_time' => $start,
            'first_output_time' => $first_output_time,
            'headers_sent_before_output' => $headers_sent_before_output,
            'time_to_first_output_ms' => ($first_output_time - $start) * 1000,
        ], JSON_PRETTY_PRINT);
        break;

    case 'sse_headers':
        // Test SSE header timing
        header('Content-Type: text/event-stream');
        header('Cache-Control: no-cache');
        header('Connection: keep-alive');
        header('X-Accel-Buffering: no');

        // Headers should be sent now via send_headers callback
        // Flush to ensure headers go out
        if (function_exists('ob_implicit_flush')) {
            ob_implicit_flush(true);
        }

        // Send a few events with timing info
        for ($i = 1; $i <= 3; $i++) {
            echo "data: {\"event\": $i, \"time\": " . microtime(true) . "}\n\n";
            flush();
            if ($i < 3) {
                usleep(100000); // 100ms delay
            }
        }
        break;

    case 'check_headers_sent':
        // Test headers_sent() function behavior
        header('Content-Type: application/json');

        $before_output = headers_sent($file, $line);

        // This output triggers header sending
        $data = [
            'status' => 'ok',
            'test' => 'check_headers_sent',
            'headers_sent_before_output' => $before_output,
            'headers_sent_file' => $file ?: 'none',
            'headers_sent_line' => $line ?: 0,
        ];

        echo json_encode($data, JSON_PRETTY_PRINT);

        // After output
        $after_output = headers_sent();
        // Can't add this to output since we already started outputting
        break;

    case 'info':
    default:
        header('Content-Type: application/json');
        echo json_encode([
            'status' => 'ok',
            'test' => 'info',
            'description' => 'Test send_headers SAPI callback',
            'available_tests' => [
                'headers_timing' => 'Test header timing',
                'sse_headers' => 'Test SSE with early headers',
                'check_headers_sent' => 'Test headers_sent() function',
            ],
        ], JSON_PRETTY_PRINT);
        break;
}
