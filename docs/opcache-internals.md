# OPcache Internals: Direct Opcode Access

Research on obtaining binary opcodes from OPcache for direct execution in Rust. For practical OPcache configuration, see [Configuration](configuration.md).

## OPcache Data Structures

### zend_persistent_script

Main structure for cached scripts:

```c
typedef struct _zend_persistent_script {
    zend_script    script;              // Compiled script
    zend_long      compiler_halt_offset;
    int            ping_auto_globals_mask;
    accel_time_t   timestamp;
    bool           corrupted;
    bool           is_phar;
    bool           empty;
    uint32_t       num_warnings;
    uint32_t       num_early_bindings;
    zend_error_info **warnings;
    zend_early_binding *early_bindings;

    void          *mem;                 // Memory pointer
    size_t         size;                // Size in shared memory

    struct {
        time_t       last_used;
        zend_ulong   hits;              // Cache hits
        unsigned int memory_consumption;
        time_t       revalidate;
    } dynamic_members;
} zend_persistent_script;
```

### zend_op_array

Opcode array for execution:

```c
struct _zend_op_array {
    uint8_t type;
    zend_string *function_name;
    zend_class_entry *scope;
    uint32_t num_args;

    uint32_t last;              // Number of opcodes
    zend_op *opcodes;           // Opcode array

    zend_string **vars;         // Local variables
    zval *literals;             // Literals (strings, numbers)

    zend_string *filename;
    uint32_t line_start;
    uint32_t line_end;

    HashTable *static_variables;
    // ...
};
```

### zend_op (single opcode)

```c
struct _zend_op {
    const void *handler;        // Handler pointer
    znode_op op1;               // First operand
    znode_op op2;               // Second operand
    znode_op result;            // Result
    uint32_t extended_value;
    uint32_t lineno;
    uint8_t opcode;             // Operation type (ZEND_ADD, ZEND_ECHO, etc.)
    uint8_t op1_type;
    uint8_t op2_type;
    uint8_t result_type;
};
```

## Opcode Execution API

### Standard Path

```c
// Compile file
zend_op_array *op_array = zend_compile_file(&file_handle, ZEND_INCLUDE);

// Execute
zval return_value;
zend_execute(op_array, &return_value);
```

### Direct Execution via execute_ex

```c
// Low-level execution
zend_execute_data *execute_data = zend_vm_stack_push_call_frame(
    ZEND_CALL_TOP_CODE, op_array, 0, NULL
);
zend_init_code_execute_data(execute_data, op_array, &return_value);
execute_ex(execute_data);
```

## Problems with Direct Access

### 1. No Public C API

OPcache does not export functions for accessing cached scripts:

```c
// Internal function (not exported)
zend_persistent_script *zend_accel_find_script(
    zend_string *filename,
    int check_timestamp
);
```

### 2. Pointers are Process-Bound

```
Shared Memory (OPcache)
┌─────────────────────────────────────┐
│ zend_persistent_script              │
│   ├── opcodes: 0x7f1234560000 ───┐  │  ← Absolute address
│   ├── literals: 0x7f1234561000   │  │
│   └── vars: 0x7f1234562000       │  │
└─────────────────────────────────────┘
                                   │
                                   ▼
                    Process A sees this address
                    Process B may mmap elsewhere!
```

### 3. Runtime Cache

```c
// Each request requires its own runtime cache
ZEND_MAP_PTR_DEF(void **, run_time_cache);  // Per-request data
```

### 4. PHP Version Dependency

Opcodes are incompatible between:
- Different PHP versions (8.3 vs 8.4)
- Different configurations (ZTS vs NTS)
- Different architectures (x86 vs ARM)

## Possible Approaches

### Approach 1: PHP Preloading

Preload scripts at server startup:

```php
// preload.php
<?php

// Load framework once
require '/var/www/vendor/autoload.php';

// Preload classes
opcache_compile_file('/var/www/app/Kernel.php');
opcache_compile_file('/var/www/app/Controller.php');
```

```bash
# php.ini
opcache.preload=/var/www/preload.php
opcache.preload_user=www-data
```

**Advantages:**
- +30-60% performance for frameworks
- Officially supported
- Classes/functions loaded once

**Limitations:**
- Requires PHP restart for updates
- Does not work in Docker without special configuration

### Approach 2: Extension for API Export

Create a PHP extension that exports internal OPcache functions:

```c
// ext/tokio_opcache_bridge.c

PHP_FUNCTION(tokio_get_cached_script)
{
    char *filename;
    size_t filename_len;

    ZEND_PARSE_PARAMETERS_START(1, 1)
        Z_PARAM_STRING(filename, filename_len)
    ZEND_PARSE_PARAMETERS_END();

    // Get cached script
    zend_string *zfilename = zend_string_init(filename, filename_len, 0);
    zend_persistent_script *script = zend_accel_find_script(zfilename, 0);

    if (!script) {
        RETURN_NULL();
    }

    // Return script information
    array_init(return_value);
    add_assoc_long(return_value, "size", script->size);
    add_assoc_long(return_value, "hits", script->dynamic_members.hits);
    add_assoc_long(return_value, "opcodes_count", script->script.main_op_array.last);

    // Memory pointer (for FFI)
    add_assoc_long(return_value, "mem_ptr", (zend_long)script->mem);
}
```

**Problem:** `zend_accel_find_script` has internal linkage, not exported.

### Approach 3: Direct Shared Memory Access

```rust
// Theoretical code - does not work directly

use std::fs::File;
use std::os::unix::io::AsRawFd;
use nix::sys::mman::{mmap, MapFlags, ProtFlags};

fn access_opcache_shm() -> Result<(), Error> {
    // OPcache uses mmap with a fixed key
    // Find via /proc/<pid>/maps

    // Problem: structures contain pointers
    // that are only valid in PHP process context
}
```

**Problem:** Pointers in structures are invalid outside PHP.

### Approach 4: op_array Caching in Extension

```c
// In tokio_sapi extension

static HashTable cached_scripts;  // Thread-local cache

void tokio_cache_script(const char *filename, zend_op_array *op_array) {
    // Copy op_array to thread-local storage
    // Use copy on next request
}

zend_op_array* tokio_get_cached_op_array(const char *filename) {
    // Return cached op_array
    // But runtime cache must be updated per-request!
}
```

**This works for immutable parts**, but:
- Runtime cache still created per-request
- Static variables per-request
- Complex synchronization

## Recommended Approach for tokio_php

See [Architecture](architecture.md) for full description.

### Current Architecture (Optimal)

```
Request → Worker Thread → php_request_startup() → execute → php_request_shutdown()
                              │
                              ▼
                    OPcache (shared memory)
                    ├── Cached op_arrays
                    ├── Interned strings
                    └── JIT compiled code
```

OPcache already does the heavy lifting:
1. Caches compiled scripts
2. Shares memory between threads
3. JIT compiles hot paths

### Optimizations Without Architecture Changes

Current OPcache settings in tokio_php (see `php.ini`):

```ini
; Current tokio_php configuration
opcache.enable = 1
opcache.enable_cli = 1
opcache.memory_consumption = 128
opcache.interned_strings_buffer = 16
opcache.max_accelerated_files = 10000
opcache.validate_timestamps = 0
opcache.revalidate_freq = 0
opcache.jit = tracing
opcache.jit_buffer_size = 64M

; Preloading - uncomment for frameworks
; opcache.preload = /var/www/html/preload.php
; opcache.preload_user = www-data
```

For larger projects, increase values:

```ini
; Production settings for large projects
opcache.memory_consumption=256
opcache.max_accelerated_files=20000
opcache.jit_buffer_size=128M
```

### Metrics for Analysis

```php
<?php

$status = opcache_get_status(true);

// Cache efficiency
$hit_rate = $status['opcache_statistics']['hits'] /
            ($status['opcache_statistics']['hits'] +
             $status['opcache_statistics']['misses']) * 100;

echo "Hit rate: {$hit_rate}%\n";
echo "Cached scripts: {$status['opcache_statistics']['num_cached_scripts']}\n";
echo "Memory used: " . round($status['memory_usage']['used_memory'] / 1024 / 1024, 2) . " MB\n";

// JIT statistics
if (isset($status['jit'])) {
    echo "JIT enabled: " . ($status['jit']['enabled'] ? 'Yes' : 'No') . "\n";
    echo "JIT buffer used: " . round($status['jit']['buffer_used'] / 1024 / 1024, 2) . " MB\n";
}
```

## Conclusions

| Approach | Feasibility | Gain | Recommendation |
|----------|-------------|------|----------------|
| PHP Preloading | High | +30-60% | Recommended |
| Bridge extension | Medium | +5-10% | Complex, risky |
| Direct SHM access | Low | - | Does not work |
| Cache in extension | Medium | +5% | Complex |

**Recommendation:** Use standard OPcache mechanisms (preloading, proper configuration, JIT) instead of trying to bypass its API.

## See Also

- [OPcache & JIT](opcache-jit.md) - Practical OPcache configuration
- [Architecture](architecture.md) - tokio_php architecture overview
- [Worker Pool](worker-pool.md) - PHP worker management
- [Configuration](configuration.md) - All environment variables
- [Profiling](profiling.md) - Performance measurement

## Sources

- [PHP OPcache Manual](https://www.php.net/manual/en/book.opcache.php)
- [How OPcache Works (Nikita Popov)](https://www.npopov.com/2021/10/13/How-opcache-works.html)
- [PHP RFC: Direct Execution Opcode](https://wiki.php.net/rfc/direct-execution-opcode) - rejected
- [PHP Preloading](https://www.php.net/manual/en/opcache.preloading.php)
- [php-src/ext/opcache](https://github.com/php/php-src/tree/master/ext/opcache)
