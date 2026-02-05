<?php
/**
 * Test tokio_http_response_code() function
 */

$action = $_GET['action'] ?? '';

switch ($action) {
    case '404':
        tokio_http_response_code(404);
        echo "Status set to 404 via tokio_http_response_code()";
        break;
    case '500':
        tokio_http_response_code(500);
        echo "Status set to 500 via tokio_http_response_code()";
        break;
    case '201':
        tokio_http_response_code(201);
        echo "Status set to 201 via tokio_http_response_code()";
        break;
    case '204':
        tokio_http_response_code(204);
        // 204 No Content - no body
        break;
    case 'get':
        $current = tokio_http_response_code();
        echo "Current status code: " . $current;
        break;
    default:
        echo "Test tokio_http_response_code() function\n";
        echo "Usage:\n";
        echo "  ?action=404 - Set status to 404\n";
        echo "  ?action=500 - Set status to 500\n";
        echo "  ?action=201 - Set status to 201\n";
        echo "  ?action=204 - Set status to 204 (no content)\n";
        echo "  ?action=get - Get current status code\n";
        break;
}
