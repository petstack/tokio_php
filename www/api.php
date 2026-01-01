<?php
/**
 * Example API endpoint demonstrating HTTP method handling.
 *
 * Supports: GET, POST, PUT, PATCH, DELETE, OPTIONS, QUERY
 *
 * Usage:
 *   GET    /api.php?id=1           - Get resource
 *   POST   /api.php                - Create resource (form or JSON)
 *   PUT    /api.php?id=1           - Replace resource (JSON body)
 *   PATCH  /api.php?id=1           - Update resource (JSON body)
 *   DELETE /api.php?id=1           - Delete resource
 *   QUERY  /api.php                - Search with JSON body
 */

header('Content-Type: application/json');

$method = $_SERVER['REQUEST_METHOD'];
$id = $_GET['id'] ?? null;

// Read raw body for PUT, PATCH, DELETE, QUERY, OPTIONS
$rawBody = file_get_contents('php://input');
$jsonBody = null;

if ($rawBody && str_contains($_SERVER['CONTENT_TYPE'] ?? '', 'application/json')) {
    $jsonBody = json_decode($rawBody, true);
}

$response = match($method) {
    'GET' => [
        'action' => 'get',
        'id' => $id,
        'message' => $id ? "Retrieved resource $id" : "List all resources",
    ],

    'POST' => [
        'action' => 'create',
        'data' => $jsonBody ?? $_POST,
        'message' => 'Resource created',
    ],

    'PUT' => [
        'action' => 'replace',
        'id' => $id,
        'data' => $jsonBody,
        'message' => $id ? "Resource $id replaced" : "Missing id parameter",
    ],

    'PATCH' => [
        'action' => 'update',
        'id' => $id,
        'data' => $jsonBody,
        'message' => $id ? "Resource $id updated" : "Missing id parameter",
    ],

    'DELETE' => [
        'action' => 'delete',
        'id' => $id,
        'message' => $id ? "Resource $id deleted" : "Missing id parameter",
    ],

    'QUERY' => [
        'action' => 'search',
        'query' => $jsonBody,
        'message' => 'Search results',
        'results' => [], // Would contain search results
    ],

    'OPTIONS' => [
        'methods' => ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS', 'QUERY'],
        'message' => 'Allowed methods',
    ],

    default => [
        'error' => 'Method not supported',
        'method' => $method,
    ],
};

// Add debug info
$response['_debug'] = [
    'method' => $method,
    'content_type' => $_SERVER['CONTENT_TYPE'] ?? null,
    'content_length' => $_SERVER['CONTENT_LENGTH'] ?? null,
    'raw_body_length' => strlen($rawBody),
];

echo json_encode($response, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE);
