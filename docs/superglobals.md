# PHP Superglobals

tokio_php provides full support for PHP superglobals, allowing PHP scripts to access request data just like with traditional PHP servers.

## Supported Superglobals

| Superglobal | Status | Description |
|-------------|--------|-------------|
| `$_GET` | Supported | URL query parameters |
| `$_POST` | Supported | Form data (urlencoded and multipart) |
| `$_SERVER` | Supported | Server and request information |
| `$_COOKIE` | Supported | HTTP cookies |
| `$_FILES` | Supported | Uploaded files |
| `$_REQUEST` | Supported | Merged GET + POST |
| `$_SESSION` | Not native | Requires custom implementation |

## $_GET

Query string parameters from the URL:

```bash
curl "http://localhost:8080/test.php?name=John&age=30"
```

```php
<?php

print_r($_GET);
// Array ( [name] => John [age] => 30 )
```

## $_POST

Form data from POST requests:

### URL-encoded Form

```bash
curl -X POST -d "name=John&email=john@example.com" \
  http://localhost:8080/form.php
```

```php
<?php

print_r($_POST);
// Array ( [name] => John [email] => john@example.com )
```

### JSON Body

For JSON requests, use `php://input`:

```bash
curl -X POST -H "Content-Type: application/json" \
  -d '{"name":"John"}' http://localhost:8080/api.php
```

```php
<?php

$json = file_get_contents('php://input');
$data = json_decode($json, true);
```

## $_FILES

File uploads via multipart/form-data:

```bash
curl -F "file=@document.pdf" http://localhost:8080/upload.php
```

```php
<?php

print_r($_FILES);
// Array (
//     [file] => Array (
//         [name] => document.pdf
//         [type] => application/pdf
//         [tmp_name] => /tmp/phpABC123
//         [error] => 0
//         [size] => 12345
//     )
// )

// Move uploaded file
move_uploaded_file($_FILES['file']['tmp_name'], '/uploads/document.pdf');
```

### Multiple File Upload

```bash
curl -F "files[]=@file1.txt" -F "files[]=@file2.txt" \
  http://localhost:8080/upload.php
```

```php
<?php

print_r($_FILES);
// Array (
//     [files] => Array (
//         [name] => Array ( [0] => file1.txt [1] => file2.txt )
//         [type] => Array ( [0] => text/plain [1] => text/plain )
//         [tmp_name] => Array ( [0] => /tmp/php... [1] => /tmp/php... )
//         [error] => Array ( [0] => 0 [1] => 0 )
//         [size] => Array ( [0] => 100 [1] => 200 )
//     )
// )
```

### Upload Limits

- **Max file size**: 10MB per file
- Files exceeding limit get `error = 1` (UPLOAD_ERR_INI_SIZE)
- Temp files are automatically cleaned up after request

## $_COOKIE

HTTP cookies from the Cookie header:

```bash
curl -b "session=abc123; user_id=42" http://localhost:8080/cookies.php
```

```php
<?php

print_r($_COOKIE);
// Array ( [session] => abc123 [user_id] => 42 )
```

## $_SERVER

Server and request information:

```php
<?php

// Request info
echo $_SERVER['REQUEST_METHOD'];    // GET, POST, etc.
echo $_SERVER['REQUEST_URI'];       // /path?query=value
echo $_SERVER['QUERY_STRING'];      // query=value
echo $_SERVER['CONTENT_TYPE'];      // application/json
echo $_SERVER['REMOTE_ADDR'];       // Client IP address
echo $_SERVER['REMOTE_PORT'];       // Client port

// Server info
echo $_SERVER['SERVER_SOFTWARE'];   // tokio_php/0.1.0
echo $_SERVER['SERVER_PROTOCOL'];   // HTTP/1.1, HTTP/2.0
echo $_SERVER['SERVER_NAME'];       // localhost
echo $_SERVER['SERVER_PORT'];       // 8080
echo $_SERVER['SERVER_ADDR'];       // 0.0.0.0
echo $_SERVER['GATEWAY_INTERFACE']; // CGI/1.1

// From headers
echo $_SERVER['HTTP_HOST'];         // localhost:8080
echo $_SERVER['HTTP_USER_AGENT'];   // curl/7.88.1
echo $_SERVER['HTTP_ACCEPT'];       // */*
echo $_SERVER['HTTP_COOKIE'];       // session=abc123
echo $_SERVER['HTTP_REFERER'];      // Previous page URL
echo $_SERVER['HTTP_ACCEPT_LANGUAGE']; // en-US,en

// Script paths
echo $_SERVER['DOCUMENT_ROOT'];     // /var/www/html
echo $_SERVER['SCRIPT_FILENAME'];   // /var/www/html/index.php
echo $_SERVER['SCRIPT_NAME'];       // /index.php
echo $_SERVER['PHP_SELF'];          // /index.php
echo $_SERVER['PATH_INFO'];         // Extra path info (if present)

// Timestamps
echo $_SERVER['REQUEST_TIME'];       // Unix timestamp
echo $_SERVER['REQUEST_TIME_FLOAT']; // With microseconds

// HTTPS (only set for TLS connections)
echo $_SERVER['HTTPS'];             // on
echo $_SERVER['SSL_PROTOCOL'];      // TLSv1.3

// Distributed tracing (W3C Trace Context)
echo $_SERVER['HTTP_TRACEPARENT'];  // 00-{trace_id}-{span_id}-01
echo $_SERVER['TRACE_ID'];          // 32-char trace identifier
echo $_SERVER['SPAN_ID'];           // 16-char span identifier
echo $_SERVER['PARENT_SPAN_ID'];    // Parent span (if propagated)

// tokio_php specific (USE_EXT=1 only)
echo $_SERVER['TOKIO_REQUEST_ID'];           // Unique request ID
echo $_SERVER['TOKIO_WORKER_ID'];            // Worker thread ID
echo $_SERVER['TOKIO_SERVER_BUILD_VERSION']; // Build version with git hash
```

See [Distributed Tracing](distributed-tracing.md) for trace context details.

## $_REQUEST

Merged array of `$_GET` and `$_POST`:

```php
<?php

// POST to /test.php?foo=1 with bar=2
print_r($_REQUEST);
// Array ( [foo] => 1 [bar] => 2 )
```

## $_SESSION

Native PHP sessions (`session_start()`) are not supported because php-embed considers headers already sent.

### Workaround: Custom Session Class

```php
<?php

class EmbedSession {
    private string $id;
    private string $path = '/tmp';

    public function start(): void {
        $this->id = $_COOKIE['PHPSESSID'] ?? bin2hex(random_bytes(16));

        if (!isset($_COOKIE['PHPSESSID'])) {
            header("Set-Cookie: PHPSESSID={$this->id}; Path=/; HttpOnly");
        }

        $file = "{$this->path}/sess_{$this->id}";
        $_SESSION = file_exists($file)
            ? unserialize(file_get_contents($file))
            : [];
    }

    public function save(): void {
        $file = "{$this->path}/sess_{$this->id}";
        file_put_contents($file, serialize($_SESSION));
    }
}

$session = new EmbedSession();
$session->start();

$_SESSION['visits'] = ($_SESSION['visits'] ?? 0) + 1;
echo "Visit #" . $_SESSION['visits'];

$session->save();
```

## Implementation Details

### FFI-based Injection (Default, USE_EXT=1)

With the tokio_sapi extension (default), superglobals are set via FFI batch API:

```c
// Single C call sets all $_SERVER variables
tokio_sapi_set_server_vars_batch(buffer, len, count);
```

FFI is **48% faster** for real applications because:
- Native `php_execute_script()` execution
- Batch API reduces FFI call overhead
- Full OPcache/JIT optimization

### Eval-based Injection (USE_EXT=0)

Legacy mode injects superglobals via `zend_eval_string()`:

```php
$_GET = ['name' => 'John'];
$_POST = [];
$_SERVER = ['REQUEST_METHOD' => 'GET', ...];
$_COOKIE = [];
$_FILES = [];
$_REQUEST = array_merge($_GET, $_POST);
```

This is simpler but slower for complex applications.

### Server Variables Optimization

Server variables (`$_SERVER`) are built with zero-allocation optimizations for common values:

| Variable | Optimization | Allocation |
|----------|--------------|------------|
| `DOCUMENT_ROOT` | Cached at server startup | Zero per request |
| `REQUEST_METHOD` | Static constants (GET, POST, PUT, DELETE, etc.) | Zero for common methods |
| `SERVER_PROTOCOL` | Static constants (HTTP/1.0, HTTP/1.1, HTTP/2.0) | Zero |
| `SERVER_SOFTWARE` | Static constant | Zero |
| `SERVER_ADDR` | Static constant ("0.0.0.0") | Zero |
| `GATEWAY_INTERFACE` | Static constant ("CGI/1.1") | Zero |

Dynamic values (like `REQUEST_URI`, `REMOTE_ADDR`, timestamps) are allocated per request.

**Performance impact**: Server variables build time reduced from ~15µs to ~6-7µs per request.

## Headers and Responses

### Setting Headers

```php
<?php

header('Content-Type: application/json');
header('X-Custom-Header: value');
http_response_code(201);
```

### Redirects

```php
<?php

header('Location: /new-page.php');
exit(); // Works correctly even with exit()
```

### Status Codes

```php
<?php

http_response_code(404);
echo "Not Found";
```

All HTTP status codes (200, 301, 302, 404, 500, etc.) are supported.

## See Also

- [HTTP Methods](http-methods.md) - GET, POST, PUT, PATCH, DELETE, OPTIONS, QUERY
- [Configuration](configuration.md) - Environment variables reference
- [Distributed Tracing](distributed-tracing.md) - W3C Trace Context support
- [Request Heartbeat](request-heartbeat.md) - Timeout extension via `tokio_request_heartbeat()`
- [tokio_sapi Extension](tokio-sapi-extension.md) - PHP extension functions
- [Architecture](architecture.md) - Request processing overview
