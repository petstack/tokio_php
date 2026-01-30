<?php
/**
 * PHP gRPC Client for tokio_php
 *
 * This example demonstrates how to call the tokio_php gRPC service
 * from PHP using the official gRPC extension.
 *
 * Requirements:
 *   pecl install grpc protobuf
 *   composer require grpc/grpc google/protobuf
 *
 * Usage:
 *   php php_client.php
 */

require_once __DIR__ . '/vendor/autoload.php';

use Google\Protobuf\Internal\Message;
use Grpc\ChannelCredentials;

// Configuration
$grpcHost = getenv('GRPC_HOST') ?: 'localhost:50051';

/**
 * Simple gRPC client without generated code
 * Uses raw gRPC calls with manual protobuf encoding
 */
class TokioPhpClient
{
    private $channel;
    private $client;

    public function __construct(string $host)
    {
        $this->channel = new \Grpc\Channel($host, [
            'credentials' => ChannelCredentials::createInsecure(),
        ]);
    }

    /**
     * Execute a PHP script via gRPC
     */
    public function execute(
        string $scriptPath,
        string $method = 'GET',
        array $queryParams = [],
        array $formData = [],
        array $cookies = [],
        ?int $timeoutMs = null
    ): array {
        // Build request using raw bytes (simplified without protobuf generation)
        $request = $this->buildExecuteRequest(
            $scriptPath,
            $method,
            $queryParams,
            $formData,
            $cookies,
            $timeoutMs
        );

        // Make gRPC call
        $call = new \Grpc\BaseStub($this->channel, []);

        list($response, $status) = $call->_simpleRequest(
            '/tokio_php.v1.PhpService/Execute',
            $request,
            [$this, 'deserializeExecuteResponse'],
            [],
            []
        )->wait();

        if ($status->code !== \Grpc\STATUS_OK) {
            throw new \RuntimeException(
                "gRPC error: {$status->details} (code: {$status->code})"
            );
        }

        return $response;
    }

    /**
     * Check service health
     */
    public function healthCheck(): array
    {
        $call = new \Grpc\BaseStub($this->channel, []);

        // Empty request for health check
        $request = '';

        list($response, $status) = $call->_simpleRequest(
            '/tokio_php.v1.PhpService/Check',
            $request,
            [$this, 'deserializeHealthResponse'],
            [],
            []
        )->wait();

        if ($status->code !== \Grpc\STATUS_OK) {
            throw new \RuntimeException(
                "gRPC error: {$status->details} (code: {$status->code})"
            );
        }

        return $response;
    }

    public function close(): void
    {
        $this->channel->close();
    }

    // Note: In production, use protoc-generated classes instead of manual serialization
    private function buildExecuteRequest(...$params): string
    {
        // This is a simplified example. In production, use:
        // protoc --php_out=. --grpc_out=. proto/php_service.proto
        return ''; // Placeholder
    }

    public function deserializeExecuteResponse($data): array
    {
        return ['raw' => $data];
    }

    public function deserializeHealthResponse($data): array
    {
        return ['status' => ord($data[0] ?? "\x00")];
    }
}

// =============================================================================
// Alternative: Using curl to call gRPC-Web or REST gateway
// =============================================================================

/**
 * Simple HTTP client that wraps gRPC calls via REST gateway (if available)
 * or demonstrates the request format
 */
class TokioPhpHttpClient
{
    private string $baseUrl;

    public function __construct(string $baseUrl = 'http://localhost:8080')
    {
        $this->baseUrl = rtrim($baseUrl, '/');
    }

    /**
     * Execute PHP script via standard HTTP (not gRPC)
     * This shows what data gRPC would send
     */
    public function execute(
        string $scriptPath,
        string $method = 'GET',
        array $queryParams = [],
        array $formData = [],
        array $headers = []
    ): array {
        $url = $this->baseUrl . '/' . ltrim($scriptPath, '/');

        if (!empty($queryParams)) {
            $url .= '?' . http_build_query($queryParams);
        }

        $ch = curl_init();
        curl_setopt_array($ch, [
            CURLOPT_URL => $url,
            CURLOPT_RETURNTRANSFER => true,
            CURLOPT_CUSTOMREQUEST => $method,
            CURLOPT_HTTPHEADER => $this->formatHeaders($headers),
            CURLOPT_TIMEOUT => 30,
        ]);

        if (!empty($formData) && in_array($method, ['POST', 'PUT', 'PATCH'])) {
            curl_setopt($ch, CURLOPT_POSTFIELDS, http_build_query($formData));
        }

        $response = curl_exec($ch);
        $httpCode = curl_getinfo($ch, CURLINFO_HTTP_CODE);
        $error = curl_error($ch);
        curl_close($ch);

        if ($error) {
            throw new \RuntimeException("HTTP error: $error");
        }

        return [
            'status_code' => $httpCode,
            'body' => $response,
        ];
    }

    private function formatHeaders(array $headers): array
    {
        $formatted = [];
        foreach ($headers as $key => $value) {
            $formatted[] = "$key: $value";
        }
        return $formatted;
    }
}

// =============================================================================
// Example Usage
// =============================================================================

echo "=== tokio_php gRPC Client Example ===\n\n";

// Using HTTP client (always works)
echo "1. HTTP Client Example:\n";
$httpClient = new TokioPhpHttpClient('http://localhost:8080');

try {
    $result = $httpClient->execute('index.php', 'GET', ['page' => '1']);
    echo "   Status: {$result['status_code']}\n";
    echo "   Body length: " . strlen($result['body']) . " bytes\n";
} catch (\Exception $e) {
    echo "   Error: " . $e->getMessage() . "\n";
}

echo "\n";

// Using gRPC client (requires grpc extension)
echo "2. gRPC Client Example:\n";
if (extension_loaded('grpc')) {
    try {
        $grpcClient = new TokioPhpClient($grpcHost);

        // Health check
        $health = $grpcClient->healthCheck();
        echo "   Health: " . json_encode($health) . "\n";

        // Execute script
        $result = $grpcClient->execute(
            'index.php',
            'GET',
            ['page' => '1', 'limit' => '10']
        );
        echo "   Result: " . json_encode($result) . "\n";

        $grpcClient->close();
    } catch (\Exception $e) {
        echo "   Error: " . $e->getMessage() . "\n";
    }
} else {
    echo "   gRPC extension not loaded. Install with: pecl install grpc\n";
}

echo "\n";

// Show what a gRPC request would look like
echo "3. gRPC Request Format (for debugging):\n";
$requestExample = [
    'script_path' => 'api/users.php',
    'method' => 'POST',
    'query_params' => ['version' => 'v1'],
    'form_data' => ['name' => 'John', 'email' => 'john@example.com'],
    'cookies' => ['session' => 'abc123'],
    'server_vars' => ['HTTP_AUTHORIZATION' => 'Bearer token123'],
    'options' => [
        'timeout_ms' => 5000,
        'enable_profiling' => false,
        'trace_parent' => '00-trace123-span456-01',
    ],
];
echo "   " . json_encode($requestExample, JSON_PRETTY_PRINT) . "\n";
