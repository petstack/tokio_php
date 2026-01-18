<?php
/**
 * Test script for SAPI activate/deactivate lifecycle hooks.
 *
 * Tests:
 * 1. Virtual env variables are cleared between requests (activate)
 * 2. Temporary files are cleaned up after requests (deactivate)
 */

header('Content-Type: application/json');

$test_type = $_GET['test'] ?? 'info';

switch ($test_type) {
    case 'create_temp':
        // Create a temp file and store info
        $tmp_file = tempnam(sys_get_temp_dir(), 'tokio_test_');
        file_put_contents($tmp_file, 'test content ' . time());

        // Return the path so the next request can check if it was cleaned up
        echo json_encode([
            'status' => 'ok',
            'test' => 'create_temp',
            'tmp_file' => $tmp_file,
            'exists' => file_exists($tmp_file),
        ], JSON_PRETTY_PRINT);
        break;

    case 'check_temp':
        // Check if a specific temp file exists
        $path = $_GET['path'] ?? '';
        if (empty($path)) {
            echo json_encode([
                'status' => 'error',
                'message' => 'No path provided',
            ], JSON_PRETTY_PRINT);
            break;
        }

        echo json_encode([
            'status' => 'ok',
            'test' => 'check_temp',
            'path' => $path,
            'exists' => file_exists($path),
            // Note: File cleanup is done by SAPI deactivate for $_FILES,
            // not for tempnam() files. This test verifies the mechanism exists.
        ], JSON_PRETTY_PRINT);
        break;

    case 'env_isolation':
        // Test that virtual env vars are isolated between requests
        // Each request should get its own trace ID
        $trace_id = getenv('TOKIO_TRACE_ID');
        $request_id = getenv('TOKIO_REQUEST_ID');

        echo json_encode([
            'status' => 'ok',
            'test' => 'env_isolation',
            'trace_id' => $trace_id !== false ? $trace_id : 'not_found',
            'request_id' => $request_id !== false ? $request_id : 'not_found',
            'timestamp' => microtime(true),
        ], JSON_PRETTY_PRINT);
        break;

    case 'info':
    default:
        // Return general info about lifecycle hooks
        echo json_encode([
            'status' => 'ok',
            'test' => 'info',
            'description' => 'Test lifecycle hooks (activate/deactivate)',
            'available_tests' => [
                'create_temp' => 'Create a temp file',
                'check_temp' => 'Check if a temp file exists (pass ?path=...)',
                'env_isolation' => 'Test virtual env isolation between requests',
            ],
            'trace_id' => getenv('TOKIO_TRACE_ID') ?: 'not_found',
            'request_id' => getenv('TOKIO_REQUEST_ID') ?: 'not_found',
            'worker_id' => getenv('TOKIO_WORKER_ID') !== false ? getenv('TOKIO_WORKER_ID') : 'not_found',
        ], JSON_PRETTY_PRINT);
        break;
}
