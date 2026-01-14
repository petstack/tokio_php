# Request Heartbeat

The `tokio_request_heartbeat()` function allows PHP scripts to extend their execution deadline, preventing 504 Gateway Timeout errors for long-running operations.

## Quick Start

```php
<?php

// Process large dataset without timeout
foreach ($large_dataset as $item) {
    process_item($item);

    // Extend both deadlines by 30 seconds
    set_time_limit(30);
    tokio_request_heartbeat(30);
}
```

## Function Signature

```php
tokio_request_heartbeat(int $time = 10): bool
```

### Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `$time` | int | 10 | Seconds to extend the deadline |

### Return Value

- `true` - Deadline successfully extended
- `false` - Extension failed (see reasons below)

### Returns `false` When

| Condition | Description |
|-----------|-------------|
| Timeout disabled | `REQUEST_TIMEOUT=off` |
| Invalid value | `$time <= 0` |
| Exceeds limit | `$time > REQUEST_TIMEOUT` (e.g., 121 > 120 for 2m timeout) |

## How It Works

```
Initial Request (REQUEST_TIMEOUT=30s)
├──────────────────────────────────────────────────┤
│                    30 seconds                    │
└──────────────────────────────────────────────────┘
                                                   ▲
                                              Timeout (504)

With Heartbeat:
├──────────────┤
│  10 seconds  │ ← Script calls tokio_request_heartbeat(15)
└──────────────┘
               ├───────────────────┤
               │    15 seconds     │ ← New deadline from NOW
               └───────────────────┘
                                   ├───────────────────┤
                                   │    15 seconds     │ ← Another heartbeat
                                   └───────────────────┘
                                                       ▲
                                                    Success!
```

### Two-Layer Timeout System

tokio_php has two independent timeout mechanisms:

1. **Rust-side deadline** - Controlled by `REQUEST_TIMEOUT`, returns 504
2. **PHP's `max_execution_time`** - Set via `set_time_limit()`, throws fatal error

`tokio_request_heartbeat()` only extends the Rust-side deadline. You must also call `set_time_limit()` to extend PHP's internal timeout.

```php
<?php

// Correct: extend both timeouts
set_time_limit(30);
tokio_request_heartbeat(30);

// Wrong: only extends Rust timeout, PHP may still timeout
tokio_request_heartbeat(30);
```

## Use Cases

### Processing Large Datasets

```php
<?php

$batch_size = 100;
$total = count($records);

for ($i = 0; $i < $total; $i += $batch_size) {
    $batch = array_slice($records, $i, $batch_size);

    foreach ($batch as $record) {
        process_record($record);
    }

    // Heartbeat after each batch
    set_time_limit(60);
    tokio_request_heartbeat(60);

    // Optional: log progress
    error_log(sprintf("Processed %d/%d records", min($i + $batch_size, $total), $total));
}

echo json_encode(['status' => 'complete', 'processed' => $total]);
```

### Slow External API Calls

```php
<?php

$endpoints = [
    'https://api.service1.com/data',
    'https://api.service2.com/data',
    'https://api.service3.com/data',
];

$results = [];

foreach ($endpoints as $url) {
    // Extend deadline before slow call
    set_time_limit(30);
    tokio_request_heartbeat(30);

    $ch = curl_init($url);
    curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);
    curl_setopt($ch, CURLOPT_TIMEOUT, 25); // Less than heartbeat

    $results[$url] = curl_exec($ch);
    curl_close($ch);
}

echo json_encode($results);
```

### File Processing

```php
<?php

$file = fopen('large_file.csv', 'r');
$line_count = 0;

while (($line = fgetcsv($file)) !== false) {
    process_csv_line($line);
    $line_count++;

    // Heartbeat every 1000 lines
    if ($line_count % 1000 === 0) {
        set_time_limit(30);
        tokio_request_heartbeat(30);
    }
}

fclose($file);
echo "Processed $line_count lines";
```

### Report Generation

```php
<?php

class ReportGenerator
{
    private int $heartbeat_interval;

    public function __construct(int $heartbeat_seconds = 30)
    {
        $this->heartbeat_interval = $heartbeat_seconds;
    }

    public function generate(): array
    {
        $sections = [
            'summary' => fn() => $this->generateSummary(),
            'details' => fn() => $this->generateDetails(),
            'charts'  => fn() => $this->generateCharts(),
            'export'  => fn() => $this->generateExport(),
        ];

        $report = [];

        foreach ($sections as $name => $generator) {
            $this->heartbeat();
            $report[$name] = $generator();
        }

        return $report;
    }

    private function heartbeat(): void
    {
        set_time_limit($this->heartbeat_interval);
        tokio_request_heartbeat($this->heartbeat_interval);
    }

    // ... section generators
}

$generator = new ReportGenerator(60);
$report = $generator->generate();
header('Content-Type: application/json');
echo json_encode($report);
```

## Configuration

### REQUEST_TIMEOUT Values

| Value | Max Heartbeat | Use Case |
|-------|---------------|----------|
| `30s` | 30 seconds | Quick API endpoints |
| `2m` | 120 seconds | Standard web requests |
| `5m` | 300 seconds | Report generation, imports |
| `10m` | 600 seconds | Heavy batch processing |
| `off` | N/A | Heartbeat disabled |

### Recommended Settings

```bash
# Standard web application
REQUEST_TIMEOUT=2m docker compose up -d

# Background job processing
REQUEST_TIMEOUT=10m docker compose up -d

# Disable timeout (not recommended for production)
REQUEST_TIMEOUT=off docker compose up -d
```

## Best Practices

### 1. Always Extend Both Timeouts

```php
<?php

function heartbeat(int $seconds = 30): bool
{
    set_time_limit($seconds);
    return tokio_request_heartbeat($seconds);
}

// Usage
heartbeat(60);
```

### 2. Use Reasonable Intervals

```php
<?php

// Good: heartbeat every 30 seconds during long operation
foreach ($items as $item) {
    process($item);
    if (++$count % 100 === 0) {
        heartbeat(30);
    }
}

// Bad: heartbeat too frequently (unnecessary overhead)
foreach ($items as $item) {
    process($item);
    heartbeat(30); // Called for every item
}

// Bad: heartbeat too infrequently (may timeout between beats)
foreach ($items as $item) {
    process($item);
    if (++$count % 10000 === 0) {
        heartbeat(30); // May timeout before reaching 10000
    }
}
```

### 3. Check Return Value for Critical Operations

```php
<?php

if (!tokio_request_heartbeat(60)) {
    // Heartbeat failed - either:
    // - REQUEST_TIMEOUT=off (no timeout configured)
    // - Value exceeds limit
    // - Invalid value (<= 0)

    error_log('Heartbeat failed, timeout may occur');
}
```

### 4. Set Heartbeat Time Less Than Work Chunk Duration

```php
<?php

// Each chunk takes ~20 seconds
$chunk_duration = 20;
$safety_margin = 10;
$heartbeat_time = $chunk_duration + $safety_margin; // 30 seconds

foreach ($chunks as $chunk) {
    set_time_limit($heartbeat_time);
    tokio_request_heartbeat($heartbeat_time);

    process_chunk($chunk); // ~20 seconds
}
```

## Monitoring

### Check if Heartbeat is Available

```php
<?php

$info = [
    'function_exists' => function_exists('tokio_request_heartbeat'),
    'server_info' => function_exists('tokio_server_info') ? tokio_server_info() : [],
];

// Test heartbeat availability
$test_result = tokio_request_heartbeat(1);
$info['heartbeat_available'] = $test_result;

header('Content-Type: application/json');
echo json_encode($info);
```

### Log Heartbeat Activity

```php
<?php

function heartbeat_with_logging(int $seconds, string $context = ''): bool
{
    $result = tokio_request_heartbeat($seconds);

    if ($result) {
        error_log(sprintf(
            '[HEARTBEAT] Extended by %ds%s',
            $seconds,
            $context ? " ($context)" : ''
        ));
    } else {
        error_log(sprintf(
            '[HEARTBEAT] Failed to extend by %ds%s',
            $seconds,
            $context ? " ($context)" : ''
        ));
    }

    set_time_limit($seconds);
    return $result;
}

// Usage
heartbeat_with_logging(60, 'batch processing');
```

## Comparison with Alternatives

### vs. Increasing REQUEST_TIMEOUT

| Approach | Pros | Cons |
|----------|------|------|
| Higher `REQUEST_TIMEOUT` | Simple, no code changes | All requests get longer timeout |
| Heartbeat | Fine-grained control, per-operation | Requires code changes |

### vs. Background Jobs

| Approach | Use Case |
|----------|----------|
| Heartbeat | Operations that must return response (reports, exports) |
| Background Jobs | Fire-and-forget operations (emails, notifications) |

```php
<?php

// Heartbeat: user waits for result
function generate_report_sync(): array
{
    // Use heartbeat to prevent timeout
    foreach ($sections as $section) {
        heartbeat(60);
        $report[] = generate_section($section);
    }
    return $report; // Return to user
}

// Background: user doesn't wait
function generate_report_async(): string
{
    $job_id = queue_job('generate_report', $params);
    return $job_id; // User polls for status
}
```

## Troubleshooting

### Heartbeat Returns False

Common reasons:

1. **`REQUEST_TIMEOUT=off`** — Timeout is disabled, heartbeat not needed
2. **`$time <= 0`** — Invalid extension value
3. **`$time > REQUEST_TIMEOUT`** — Extension exceeds configured limit

```php
<?php

// Check why heartbeat returns false
$result = tokio_request_heartbeat(60);

if (!$result) {
    // Check if function exists
    if (!function_exists('tokio_request_heartbeat')) {
        echo "tokio_sapi extension not loaded.\n";
    } else {
        // Likely causes:
        // - REQUEST_TIMEOUT=off (no timeout configured)
        // - Value too large (exceeds REQUEST_TIMEOUT limit)
        // - Value <= 0
        echo "Heartbeat failed. Check REQUEST_TIMEOUT setting.\n";
    }
}
```

### Still Getting 504 Timeout

1. **Check PHP timeout**: Are you also calling `set_time_limit()`?

```php
// Wrong
tokio_request_heartbeat(60);

// Correct
set_time_limit(60);
tokio_request_heartbeat(60);
```

2. **Check heartbeat value**: Is it within the limit?

```php
// If REQUEST_TIMEOUT=2m (120s)
tokio_request_heartbeat(121); // Returns false!
tokio_request_heartbeat(120); // OK
```

3. **Check heartbeat frequency**: Are you calling it often enough?

```php
// If heartbeat(30) but operation takes 40 seconds between calls
// = timeout will occur

// Solution: more frequent heartbeats
if (++$count % 50 === 0) {  // More frequently
    heartbeat(30);
}
```

### Fatal Error: Maximum Execution Time Exceeded

This is PHP's internal timeout, not tokio_php's. Make sure to call `set_time_limit()`:

```php
// Add this before tokio_request_heartbeat()
set_time_limit(60);
tokio_request_heartbeat(60);
```

## Technical Details

### Implementation

The heartbeat mechanism uses the bridge library (`libtokio_bridge.so`) for communication between PHP and Rust:

```
PHP Script                    Bridge (TLS)                 Rust Runtime
     │                             │                             │
     │ tokio_request_heartbeat(30) │                             │
     │────────────────────────────►│                             │
     │                             │ tokio_bridge_send_heartbeat │
     │                             │────────────────────────────►│
     │                             │                             │ Update AtomicU64
     │                             │                             │ deadline_ms
     │                             │◄────────────────────────────│
     │◄────────────────────────────│ return success/failure      │
     │                             │                             │
```

1. **Rust** registers heartbeat context and callback in bridge TLS at request start
2. **PHP** calls `tokio_request_heartbeat()` which invokes `tokio_bridge_send_heartbeat()`
3. **Bridge** invokes the Rust callback, which atomically updates the deadline
4. **Rust** async loop checks deadline and returns 504 if exceeded

See [Bridge Architecture](architecture.md#bridge-architecture) for details.

### HeartbeatContext Internals

```rust
pub struct HeartbeatContext {
    start: Instant,           // Reused from request queued_at (no extra syscall)
    deadline_ms: AtomicU64,   // Milliseconds from start
    max_extension_secs: u64,  // Maximum allowed extension
}
```

Uses `Instant`-based timing instead of `SystemTime` to avoid syscall overhead. The `start` field is reused from `queued_at`, meaning no additional `Instant::now()` call is needed when creating the context.

### Thread Safety

- `HeartbeatContext` uses `AtomicU64` for atomic deadline updates
- Safe to call from any PHP code (extensions, frameworks, etc.)
- Each request has its own isolated context

### Performance

- Heartbeat call overhead: < 1 microsecond
- Uses `Instant::elapsed()` (single syscall per check)
- Memory: ~32 bytes per request for HeartbeatContext (Instant + AtomicU64 + u64)

## See Also

- [Configuration](configuration.md) - `REQUEST_TIMEOUT` setting
- [tokio_sapi Extension](tokio-sapi-extension.md) - Other PHP functions
- [Worker Pool](worker-pool.md) - Request execution model
- [Architecture](architecture.md) - System design overview
- [Profiling](profiling.md) - Request timing analysis
