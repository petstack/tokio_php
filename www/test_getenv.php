<?php
/**
 * Test script for getenv() integration with SAPI getenv callback.
 *
 * Tests that PHP's getenv() function can access virtual environment
 * variables set by tokio_php without polluting the real process environment.
 */

header('Content-Type: application/json');

// Test virtual environment variables via getenv()
$tokio_request_id = getenv('TOKIO_REQUEST_ID');
$tokio_worker_id = getenv('TOKIO_WORKER_ID');
$tokio_trace_id = getenv('TOKIO_TRACE_ID');
$tokio_span_id = getenv('TOKIO_SPAN_ID');

// Also test $_SERVER for comparison
$server_request_id = $_SERVER['TOKIO_REQUEST_ID'] ?? 'not_set';
$server_trace_id = $_SERVER['TRACE_ID'] ?? 'not_set';

// Test real environment variable (should still work)
$path = getenv('PATH');
$has_real_env = !empty($path);

// Test non-existent variable (should return false)
$non_existent = getenv('TOKIO_NON_EXISTENT_VAR_12345');

// Return test results
$data = [
    'status' => 'ok',
    'test' => 'getenv',
    'virtual_env' => [
        'TOKIO_REQUEST_ID' => $tokio_request_id !== false ? $tokio_request_id : 'not_found',
        'TOKIO_WORKER_ID' => $tokio_worker_id !== false ? $tokio_worker_id : 'not_found',
        'TOKIO_TRACE_ID' => $tokio_trace_id !== false ? $tokio_trace_id : 'not_found',
        'TOKIO_SPAN_ID' => $tokio_span_id !== false ? $tokio_span_id : 'not_found',
    ],
    'server_vars' => [
        'TOKIO_REQUEST_ID' => $server_request_id,
        'TRACE_ID' => $server_trace_id,
    ],
    'real_env_works' => $has_real_env,
    'non_existent_is_false' => $non_existent === false,
    'checks' => [
        'request_id_via_getenv' => $tokio_request_id !== false,
        'worker_id_via_getenv' => $tokio_worker_id !== false,
        'trace_id_via_getenv' => $tokio_trace_id !== false,
        'span_id_via_getenv' => $tokio_span_id !== false,
        'trace_id_matches_server' => $tokio_trace_id === $server_trace_id,
    ],
];

echo json_encode($data, JSON_PRETTY_PRINT);
