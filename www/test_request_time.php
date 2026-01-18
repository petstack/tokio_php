<?php
/**
 * Test script for $_SERVER['REQUEST_TIME'] and $_SERVER['REQUEST_TIME_FLOAT']
 *
 * Verifies that SAPI get_request_time callback works correctly.
 */

header('Content-Type: application/json');

$now = microtime(true);
$request_time = $_SERVER['REQUEST_TIME'] ?? 0;
$request_time_float = $_SERVER['REQUEST_TIME_FLOAT'] ?? 0.0;

// REQUEST_TIME should be an integer (Unix timestamp)
// REQUEST_TIME_FLOAT should be a float with microsecond precision
// Both should be close to current time (within a few seconds)

$data = [
    'request_time' => (int)$request_time,
    'request_time_float' => (float)$request_time_float,
    'current_time' => $now,
    'delay_ms' => ($now - $request_time_float) * 1000,
    'is_valid' => (
        $request_time > 0 &&
        $request_time_float > 0 &&
        abs($now - $request_time_float) < 5 // Within 5 seconds
    )
];

echo json_encode($data, JSON_PRETTY_PRINT);
