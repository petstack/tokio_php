<?php
/**
 * Test script for error_log() integration with SAPI log_message callback.
 *
 * Tests that PHP's error_log() function routes to our structured JSON logger
 * with trace correlation (request_id, trace_id, span_id).
 */

header('Content-Type: application/json');

// Get trace context from $_SERVER for verification
$request_id = $_SERVER['TOKIO_REQUEST_ID'] ?? 'unknown';
$trace_id = $_SERVER['TRACE_ID'] ?? 'unknown';
$span_id = $_SERVER['SPAN_ID'] ?? 'unknown';

// Log a test message - should appear in tokio_php logs with trace context
error_log("Test log message from PHP script");
error_log("Request ID: $request_id, Trace ID: $trace_id");

// Also test trigger_error for different levels
trigger_error("This is a notice", E_USER_NOTICE);
trigger_error("This is a warning", E_USER_WARNING);

// Return success response
$data = [
    'status' => 'ok',
    'test' => 'error_log',
    'request_id' => $request_id,
    'trace_id' => $trace_id,
    'span_id' => $span_id,
    'message' => 'Check server logs for test log messages'
];

echo json_encode($data, JSON_PRETTY_PRINT);
