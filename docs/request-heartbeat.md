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
├─────────────────────────────────────────────────┤
│                    30 seconds                    │
└─────────────────────────────────────────────────┘
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
    'timeout_configured' => !empty($_SERVER['TOKIO_HEARTBEAT_CTX']),
    'max_extension' => $_SERVER['TOKIO_HEARTBEAT_MAX_SECS'] ?? 'N/A',
];

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

```php
<?php
// Debug heartbeat issues
$debug = [
    'heartbeat_ctx' => $_SERVER['TOKIO_HEARTBEAT_CTX'] ?? null,
    'max_secs' => $_SERVER['TOKIO_HEARTBEAT_MAX_SECS'] ?? null,
    'callback' => $_SERVER['TOKIO_HEARTBEAT_CALLBACK'] ?? null,
];

if (!$debug['heartbeat_ctx']) {
    echo "Heartbeat not configured. Check REQUEST_TIMEOUT setting.\n";
} elseif (!$debug['max_secs'] || $debug['max_secs'] === '0') {
    echo "Timeout is disabled (REQUEST_TIMEOUT=off).\n";
} else {
    echo "Heartbeat available. Max extension: {$debug['max_secs']} seconds.\n";
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

The heartbeat mechanism uses three `$_SERVER` variables set by Rust:

| Variable | Description |
|----------|-------------|
| `TOKIO_HEARTBEAT_CTX` | Hex pointer to HeartbeatContext struct |
| `TOKIO_HEARTBEAT_MAX_SECS` | Maximum allowed extension (= REQUEST_TIMEOUT) |
| `TOKIO_HEARTBEAT_CALLBACK` | Hex pointer to Rust callback function |

The PHP function reads these values and calls the Rust callback via FFI, which atomically updates the deadline in the async polling loop.

### Thread Safety

- `HeartbeatContext` uses `AtomicU64` for the deadline
- Safe to call from any PHP code (extensions, frameworks, etc.)
- Each request has its own isolated context

### Performance

- Heartbeat call overhead: < 1 microsecond
- No network I/O or system calls
- Memory: ~24 bytes per request for HeartbeatContext

## See Also

- [Configuration](configuration.md) - `REQUEST_TIMEOUT` setting
- [tokio_sapi Extension](tokio-sapi-extension.md) - Other PHP functions
- [Worker Pool](worker-pool.md) - Request execution model
