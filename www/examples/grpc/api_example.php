<?php
/**
 * Example API endpoint that can be called via gRPC or HTTP
 *
 * This script demonstrates how a PHP API works the same way
 * whether called via HTTP or gRPC.
 *
 * HTTP:
 *   curl -X POST http://localhost:8080/examples/grpc/api_example.php \
 *       -d "name=John&email=john@example.com"
 *
 * gRPC:
 *   grpcurl -plaintext -d '{
 *     "script_path": "examples/grpc/api_example.php",
 *     "method": "POST",
 *     "form_data": {"name": "John", "email": "john@example.com"}
 *   }' localhost:50051 tokio_php.v1.PhpService/Execute
 */

header('Content-Type: application/json');

// Get request data (works for both HTTP and gRPC)
$method = $_SERVER['REQUEST_METHOD'] ?? 'GET';
$requestId = $_SERVER['HTTP_X_REQUEST_ID'] ?? ($_SERVER['REQUEST_ID'] ?? uniqid());

// Response helper
function jsonResponse(int $status, array $data): void
{
    http_response_code($status);
    echo json_encode($data, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE);
}

// Route by method
switch ($method) {
    case 'GET':
        // List users (example)
        $page = (int)($_GET['page'] ?? 1);
        $limit = (int)($_GET['limit'] ?? 10);

        jsonResponse(200, [
            'success' => true,
            'request_id' => $requestId,
            'data' => [
                'users' => [
                    ['id' => 1, 'name' => 'Alice', 'email' => 'alice@example.com'],
                    ['id' => 2, 'name' => 'Bob', 'email' => 'bob@example.com'],
                ],
                'pagination' => [
                    'page' => $page,
                    'limit' => $limit,
                    'total' => 2,
                ],
            ],
            'meta' => [
                'server' => 'tokio_php',
                'version' => $_SERVER['TOKIO_SERVER_BUILD_VERSION'] ?? '0.1.0',
                'worker_id' => tokio_worker_id(),
            ],
        ]);
        break;

    case 'POST':
        // Create user
        $name = $_POST['name'] ?? null;
        $email = $_POST['email'] ?? null;

        // Validate
        if (empty($name) || empty($email)) {
            jsonResponse(400, [
                'success' => false,
                'request_id' => $requestId,
                'error' => [
                    'code' => 'VALIDATION_ERROR',
                    'message' => 'Name and email are required',
                    'fields' => [
                        'name' => empty($name) ? 'Required' : null,
                        'email' => empty($email) ? 'Required' : null,
                    ],
                ],
            ]);
            break;
        }

        // Simulate user creation
        $userId = rand(1000, 9999);

        jsonResponse(201, [
            'success' => true,
            'request_id' => $requestId,
            'data' => [
                'id' => $userId,
                'name' => $name,
                'email' => $email,
                'created_at' => date('c'),
            ],
        ]);
        break;

    case 'PUT':
    case 'PATCH':
        // Update user
        $body = file_get_contents('php://input');
        $data = json_decode($body, true) ?? [];

        jsonResponse(200, [
            'success' => true,
            'request_id' => $requestId,
            'data' => [
                'updated' => true,
                'fields' => array_keys($data),
            ],
        ]);
        break;

    case 'DELETE':
        // Delete user
        jsonResponse(200, [
            'success' => true,
            'request_id' => $requestId,
            'data' => [
                'deleted' => true,
            ],
        ]);
        break;

    default:
        jsonResponse(405, [
            'success' => false,
            'request_id' => $requestId,
            'error' => [
                'code' => 'METHOD_NOT_ALLOWED',
                'message' => "Method $method is not allowed",
                'allowed' => ['GET', 'POST', 'PUT', 'PATCH', 'DELETE'],
            ],
        ]);
}
