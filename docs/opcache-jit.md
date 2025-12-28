# OPcache & JIT

tokio_php supports PHP's OPcache and JIT compiler, providing significant performance improvements.

## Performance Impact

| Configuration | Requests/sec | Latency | Improvement |
|---------------|--------------|---------|-------------|
| No OPcache | ~12,400 | 8.27ms | baseline |
| OPcache | ~22,760 | 5.40ms | +84% |
| OPcache + JIT | ~23,650 | 4.46ms | +91% |

## How It Works

### OPcache

OPcache caches compiled PHP bytecode in shared memory:

1. First request: Parse PHP → Compile to opcodes → Store in shared memory
2. Subsequent requests: Load opcodes directly from shared memory

Benefits:
- Eliminates parsing and compilation overhead
- Shared across all worker threads
- Validates timestamps disabled for production (no stat() calls)

### JIT (Just-In-Time Compilation)

JIT compiles hot PHP code paths to native machine code:

1. **Tracing mode**: Profiles code execution to find hot paths
2. **Compilation**: Compiles frequently executed loops/functions to native code
3. **Execution**: Runs native code instead of interpreting opcodes

Example performance on CPU-intensive code:

```php
<?php

function fibonacci($n) {
    if ($n <= 1) return $n;
    return fibonacci($n - 1) + fibonacci($n - 2);
}

$result = fibonacci(20);
// Without JIT: 1.5ms
// With JIT:    0.38ms (4x faster)
```

## Configuration

### Dockerfile Settings

```dockerfile
RUN echo "zend_extension=opcache" >> /etc/php84/php.ini && \
    echo "opcache.enable=1" >> /etc/php84/php.ini && \
    echo "opcache.enable_cli=1" >> /etc/php84/php.ini && \
    echo "opcache.memory_consumption=128" >> /etc/php84/php.ini && \
    echo "opcache.interned_strings_buffer=16" >> /etc/php84/php.ini && \
    echo "opcache.max_accelerated_files=10000" >> /etc/php84/php.ini && \
    echo "opcache.validate_timestamps=0" >> /etc/php84/php.ini && \
    echo "opcache.jit_buffer_size=64M" >> /etc/php84/php.ini && \
    echo "opcache.jit=tracing" >> /etc/php84/php.ini
```

### Configuration Options

| Option | Value | Description |
|--------|-------|-------------|
| `opcache.enable` | 1 | Enable OPcache |
| `opcache.enable_cli` | 1 | Enable for CLI/embed SAPI |
| `opcache.memory_consumption` | 128 | Memory for opcodes (MB) |
| `opcache.interned_strings_buffer` | 16 | Memory for strings (MB) |
| `opcache.max_accelerated_files` | 10000 | Max cached scripts |
| `opcache.validate_timestamps` | 0 | Don't check file changes |
| `opcache.jit_buffer_size` | 64M | Memory for JIT code |
| `opcache.jit` | tracing | JIT mode |

### JIT Modes

| Mode | Description |
|------|-------------|
| `off` | JIT disabled |
| `tracing` | Profile and compile hot traces (recommended) |
| `function` | Compile entire functions |

## SAPI Name Trick

OPcache normally disables itself for the "embed" SAPI, considering it for short-lived CLI scripts. tokio_php works around this by changing the SAPI name to "cli-server" before initialization:

```rust
// In src/executor/sapi.rs
php_embed_module.name = "cli-server\0".as_ptr() as *mut c_char;
php_embed_init(...);
```

This technique was borrowed from NGINX Unit's PHP implementation.

## Checking Status

### OPcache Status

```php
<?php

$status = opcache_get_status();
echo "OPcache enabled: " . ($status['opcache_enabled'] ? 'Yes' : 'No') . "\n";
echo "Cached scripts: " . $status['opcache_statistics']['num_cached_scripts'] . "\n";
echo "Cache hits: " . $status['opcache_statistics']['hits'] . "\n";
echo "Memory used: " . round($status['memory_usage']['used_memory'] / 1024 / 1024, 2) . " MB\n";

```

### JIT Status

```php
<?php

$status = opcache_get_status();
$jit = $status['jit'];
echo "JIT enabled: " . ($jit['enabled'] ? 'Yes' : 'No') . "\n";
echo "JIT on: " . ($jit['on'] ? 'Yes' : 'No') . "\n";
echo "JIT buffer size: " . round($jit['buffer_size'] / 1024 / 1024, 2) . " MB\n";
echo "JIT buffer used: " . round($jit['buffer_used'] / 1024 / 1024, 2) . " MB\n";
```

### phpinfo()

```php
<?php

// Shows full OPcache and JIT configuration
phpinfo();
```

## Preloading

PHP 7.4+ supports preloading - loading scripts once at startup for all requests.

### preload.php

```php
<?php
// /var/www/html/preload.php

// Preload framework autoloader
require __DIR__ . '/vendor/autoload.php';

// Preload specific classes
opcache_compile_file(__DIR__ . '/src/Kernel.php');
opcache_compile_file(__DIR__ . '/src/Controller/BaseController.php');

// Preload entire directory
function preloadDir(string $path): void {
    $iterator = new RecursiveIteratorIterator(
        new RecursiveDirectoryIterator($path)
    );
    foreach ($iterator as $file) {
        if ($file->getExtension() === 'php') {
            opcache_compile_file($file->getPathname());
        }
    }
}

preloadDir(__DIR__ . '/src/');
```

### Configuration

```ini
; Enable preloading
opcache.preload=/var/www/html/preload.php
opcache.preload_user=www-data
```

### Benefits

- Eliminates compilation time for preloaded files
- Classes are "linked" at startup (faster autoloading)
- Shared memory across all workers
- +30-60% performance for frameworks

### Limitations

- Requires server restart to update preloaded code
- Only works with cli-server SAPI (tokio_php uses this)
- Cannot preload files that define functions/classes conditionally

## Best Practices

### Production Settings

```ini
; Disable timestamp validation (faster, no stat() calls)
opcache.validate_timestamps=0

; Increase memory for large codebases
opcache.memory_consumption=256
opcache.max_accelerated_files=20000

; Enable JIT with large buffer
opcache.jit=tracing
opcache.jit_buffer_size=128M

; Preloading
opcache.preload=/var/www/html/preload.php
opcache.preload_user=root
```

### Development Settings

```ini
; Enable timestamp validation for hot reload
opcache.validate_timestamps=1
opcache.revalidate_freq=0

; JIT can be disabled for debugging
opcache.jit=off
```

### Clearing Cache

OPcache can be cleared via PHP:

```php
<?php

opcache_reset(); // Clear entire cache
opcache_invalidate('/path/to/file.php', true); // Invalidate specific file
```

Note: With `validate_timestamps=0`, you must restart the server or call `opcache_reset()` after code changes.

## Troubleshooting

### OPcache Not Enabled

Check SAPI name:
```php
<?php

echo php_sapi_name(); // Should be "cli-server", not "embed"
```

### JIT Not Working

1. Check JIT status:
```php
<?php

var_dump(opcache_get_status()['jit']);
```

2. Ensure buffer size is set:
```ini
opcache.jit_buffer_size=64M
```

3. JIT requires a supported CPU architecture (x86-64 or ARM64)

### Memory Issues

If you see "Unable to reattach to base address" errors, increase shared memory limits in the container or reduce `opcache.memory_consumption`.
