<?php
/**
 * Test script for tokio_sapi extension
 */

header('Content-Type: application/json');

$result = [
    'extension_loaded' => extension_loaded('tokio_sapi'),
    'functions' => [
        'tokio_request_id' => function_exists('tokio_request_id'),
        'tokio_worker_id' => function_exists('tokio_worker_id'),
        'tokio_server_info' => function_exists('tokio_server_info'),
        'tokio_async_call' => function_exists('tokio_async_call'),
    ],
    'constants' => [
        'TOKIO_SAPI_VERSION' => defined('TOKIO_SAPI_VERSION') ? TOKIO_SAPI_VERSION : null,
    ],
    'server_tokio_vars' => array_filter($_SERVER, fn($k) => strpos($k, 'TOKIO') !== false, ARRAY_FILTER_USE_KEY),
];

if (function_exists('tokio_server_info')) {
    $result['server_info'] = tokio_server_info();
}

if (function_exists('tokio_request_id')) {
    $result['request_id'] = tokio_request_id();
}

if (function_exists('tokio_worker_id')) {
    $result['worker_id'] = tokio_worker_id();
}

echo json_encode($result, JSON_PRETTY_PRINT);
