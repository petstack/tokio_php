# HTTP Methods

tokio_php supports all standard HTTP methods, enabling you to build RESTful APIs and handle various request types.

## Supported Methods

| Method | Request Body | `php://input` | `$_POST`* | Description |
|--------|--------------|---------------|-----------|-------------|
| GET | No | — | — | Retrieve resource |
| HEAD | No | — | — | Get headers only |
| POST | Yes | ✓ | ✓ | Create resource |
| PUT | Yes | ✓ | — | Replace resource |
| PATCH | Yes | ✓ | — | Partial update |
| DELETE | Optional | ✓ | — | Delete resource |
| OPTIONS | Optional | ✓ | — | Get allowed methods |
| QUERY | Yes | ✓ | ✓ | Safe search with body |

*`$_POST` is populated for any method with `application/x-www-form-urlencoded` or `multipart/form-data` body. Typically only POST and QUERY use these content types; other methods use JSON via `php://input`.

## Reading Request Body

Use the standard PHP `php://input` stream to read raw request body:

```php
<?php
// Read raw body (works for all methods with body)
$rawBody = file_get_contents('php://input');

// Parse JSON
$data = json_decode($rawBody, true);

// Get content info
$contentType = $_SERVER['CONTENT_TYPE'] ?? '';
$contentLength = $_SERVER['CONTENT_LENGTH'] ?? 0;
```

For `application/x-www-form-urlencoded` or `multipart/form-data` POST requests, data is also available in `$_POST`:

```php
// Form data (POST only with urlencoded/multipart)
$name = $_POST['name'] ?? '';
$email = $_POST['email'] ?? '';
```

## REST API Example

```php
<?php
// api.php - RESTful API endpoint

header('Content-Type: application/json');

$method = $_SERVER['REQUEST_METHOD'];
$id = $_GET['id'] ?? null;

// Read and parse JSON body
$body = file_get_contents('php://input');
$data = null;
if ($body && str_contains($_SERVER['CONTENT_TYPE'] ?? '', 'application/json')) {
    $data = json_decode($body, true);
}

$response = match($method) {
    'GET' => handleGet($id),
    'POST' => handleCreate($data),
    'PUT' => handleReplace($id, $data),
    'PATCH' => handleUpdate($id, $data),
    'DELETE' => handleDelete($id),
    'OPTIONS' => handleOptions(),
    'QUERY' => handleSearch($data),
    default => ['error' => 'Method not allowed'],
};

echo json_encode($response);

function handleGet(?string $id): array {
    if ($id) {
        // Get single resource
        return ['id' => $id, 'name' => 'Example'];
    }
    // List all resources
    return ['items' => []];
}

function handleCreate(?array $data): array {
    // Validate and create
    return ['id' => uniqid(), 'created' => true, 'data' => $data];
}

function handleReplace(?string $id, ?array $data): array {
    if (!$id) {
        http_response_code(400);
        return ['error' => 'Missing id'];
    }
    return ['id' => $id, 'replaced' => true, 'data' => $data];
}

function handleUpdate(?string $id, ?array $data): array {
    if (!$id) {
        http_response_code(400);
        return ['error' => 'Missing id'];
    }
    return ['id' => $id, 'updated' => true, 'data' => $data];
}

function handleDelete(?string $id): array {
    if (!$id) {
        http_response_code(400);
        return ['error' => 'Missing id'];
    }
    return ['id' => $id, 'deleted' => true];
}

function handleOptions(): array {
    header('Allow: GET, POST, PUT, PATCH, DELETE, OPTIONS, QUERY');
    return ['methods' => ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS', 'QUERY']];
}

function handleSearch(?array $query): array {
    // Execute search based on query
    return ['query' => $query, 'results' => []];
}
```

## Testing Methods

```bash
# GET - retrieve resource
curl http://localhost:8080/api.php?id=123

# POST - create with JSON
curl -X POST -H "Content-Type: application/json" \
  -d '{"name":"Test","email":"test@example.com"}' \
  http://localhost:8080/api.php

# POST - create with form data
curl -X POST -d "name=Test&email=test@example.com" \
  http://localhost:8080/api.php

# PUT - replace resource
curl -X PUT -H "Content-Type: application/json" \
  -d '{"name":"Updated","email":"new@example.com"}' \
  http://localhost:8080/api.php?id=123

# PATCH - partial update
curl -X PATCH -H "Content-Type: application/json" \
  -d '{"status":"active"}' \
  http://localhost:8080/api.php?id=123

# DELETE - remove resource
curl -X DELETE http://localhost:8080/api.php?id=123

# OPTIONS - get allowed methods
curl -X OPTIONS http://localhost:8080/api.php

# QUERY - search with body
curl -X QUERY -H "Content-Type: application/json" \
  -d '{"search":"keyword","limit":10}' \
  http://localhost:8080/api.php
```

## HTTP QUERY Method

The QUERY method is defined in [RFC draft](https://httpwg.org/http-extensions/draft-ietf-httpbis-safe-method-w-body.html) and provides a safe, idempotent way to send complex queries in the request body.

### Properties

- **Safe** — Does not modify server state (like GET)
- **Idempotent** — Multiple identical requests have same effect
- **Cacheable** — Responses can be cached (unlike POST)
- **Body support** — Can include query parameters in body

### Use Cases

QUERY is ideal when:
- Query parameters are too long for URL (GET has ~2000 char limit)
- Query contains complex structured data (nested JSON)
- Query includes sensitive data (not logged in URL)

```php
<?php
// Handle QUERY method for advanced search
if ($_SERVER['REQUEST_METHOD'] === 'QUERY') {
    $query = json_decode(file_get_contents('php://input'), true);

    $results = search([
        'keywords' => $query['keywords'] ?? [],
        'filters' => $query['filters'] ?? [],
        'sort' => $query['sort'] ?? 'relevance',
        'page' => $query['page'] ?? 1,
        'limit' => min($query['limit'] ?? 20, 100),
    ]);

    echo json_encode(['results' => $results]);
}
```

## CORS Handling

For cross-origin requests, handle OPTIONS preflight:

```php
<?php
// Handle CORS preflight
if ($_SERVER['REQUEST_METHOD'] === 'OPTIONS') {
    header('Access-Control-Allow-Origin: *');
    header('Access-Control-Allow-Methods: GET, POST, PUT, PATCH, DELETE, OPTIONS, QUERY');
    header('Access-Control-Allow-Headers: Content-Type, Authorization');
    header('Access-Control-Max-Age: 86400');
    exit;
}

// Set CORS headers for actual requests
header('Access-Control-Allow-Origin: *');
```

## Method-Specific Headers

### Content-Type

Always set appropriate `Content-Type` for requests with body:

```bash
# JSON
-H "Content-Type: application/json"

# Form data
-H "Content-Type: application/x-www-form-urlencoded"

# XML
-H "Content-Type: application/xml"
```

### Accept

Specify expected response format:

```bash
-H "Accept: application/json"
```

## Error Handling

Return appropriate HTTP status codes:

```php
<?php
$method = $_SERVER['REQUEST_METHOD'];

// Method not allowed
if (!in_array($method, ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS', 'QUERY'])) {
    http_response_code(405);
    header('Allow: GET, POST, PUT, PATCH, DELETE, OPTIONS, QUERY');
    echo json_encode(['error' => 'Method not allowed']);
    exit;
}

// Missing required parameter
if (in_array($method, ['PUT', 'PATCH', 'DELETE']) && empty($_GET['id'])) {
    http_response_code(400);
    echo json_encode(['error' => 'Missing id parameter']);
    exit;
}

// Invalid JSON body
$body = file_get_contents('php://input');
if ($body && str_contains($_SERVER['CONTENT_TYPE'] ?? '', 'application/json')) {
    $data = json_decode($body, true);
    if (json_last_error() !== JSON_ERROR_NONE) {
        http_response_code(400);
        echo json_encode(['error' => 'Invalid JSON: ' . json_last_error_msg()]);
        exit;
    }
}
```

## Comparison with Other Servers

| Feature | tokio_php | nginx + PHP-FPM | Apache + mod_php |
|---------|-----------|-----------------|------------------|
| GET | ✓ | ✓ | ✓ |
| POST | ✓ | ✓ | ✓ |
| PUT | ✓ | ✓ | ✓ |
| PATCH | ✓ | ✓ | ✓ |
| DELETE | ✓ | ✓ | ✓ |
| OPTIONS | ✓ | ✓ | ✓ |
| HEAD | ✓ | ✓ | ✓ |
| QUERY | ✓ | — | — |
| php://input | ✓ | ✓ | ✓ |

tokio_php is the first PHP server to natively support the HTTP QUERY method.

## See Also

- [Superglobals](superglobals.md) — `$_GET`, `$_POST`, `$_SERVER` documentation
- [Single Entry Point](single-entry-point.md) — Laravel/Symfony routing
- [Configuration](configuration.md) — Server settings
