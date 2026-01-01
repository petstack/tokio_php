<?php
/**
 * Monolog formatter matching tokio_php JSON log format.
 *
 * Copy this file to your project: src/Logging/TokioPhpFormatter.php
 *
 * Usage:
 *   $handler = new StreamHandler('php://stderr');
 *   $handler->setFormatter(new TokioPhpFormatter('myapp'));
 *   $logger->pushHandler($handler);
 *
 * Output format:
 *   {"ts":"2025-01-15T10:30:00.123Z","level":"info","type":"app","msg":"...","ctx":{...},"data":{...}}
 *
 * @see docs/logging.md for full documentation
 */

namespace App\Logging;

use Monolog\Formatter\JsonFormatter;
use Monolog\LogRecord;

class TokioPhpFormatter extends JsonFormatter
{
    private string $service;

    public function __construct(string $service = 'app')
    {
        parent::__construct();
        $this->service = $service;
    }

    public function format(LogRecord $record): string
    {
        $context = $record->context;
        $extra = $record->extra;

        // Build ctx object
        $ctx = [
            'service' => $this->service,
        ];

        // Add request_id from $_SERVER if available
        if (isset($_SERVER['TOKIO_REQUEST_ID'])) {
            $ctx['request_id'] = $_SERVER['TOKIO_REQUEST_ID'];
        } elseif (isset($_SERVER['HTTP_X_REQUEST_ID'])) {
            $ctx['request_id'] = $_SERVER['HTTP_X_REQUEST_ID'];
        }

        // Add trace context if available
        if (isset($_SERVER['TRACE_ID'])) {
            $ctx['trace_id'] = $_SERVER['TRACE_ID'];
        }
        if (isset($_SERVER['SPAN_ID'])) {
            $ctx['span_id'] = $_SERVER['SPAN_ID'];
        }

        // Move known context fields to ctx
        foreach (['request_id', 'trace_id', 'span_id', 'user_id'] as $field) {
            if (isset($context[$field])) {
                $ctx[$field] = $context[$field];
                unset($context[$field]);
            }
        }

        // Determine log type
        $type = $context['type'] ?? 'app';
        unset($context['type']);

        // Build data object from remaining context
        $data = array_merge($context, $extra);

        // Handle exception
        if (isset($data['exception']) && $data['exception'] instanceof \Throwable) {
            $e = $data['exception'];
            $data['exception'] = [
                'class' => get_class($e),
                'message' => $e->getMessage(),
                'code' => $e->getCode(),
                'file' => $e->getFile(),
                'line' => $e->getLine(),
                'trace' => array_slice($e->getTrace(), 0, 10),
            ];
        }

        $output = [
            'ts' => $record->datetime->format('Y-m-d\TH:i:s.v\Z'),
            'level' => strtolower($record->level->name),
            'type' => $type,
            'msg' => $record->message,
            'ctx' => $ctx,
            'data' => (object)$data, // Force {} for empty
        ];

        return $this->toJson($output) . "\n";
    }
}

/*
 * ============================================================================
 * Laravel config/logging.php example:
 * ============================================================================
 *
 * 'channels' => [
 *     'tokio' => [
 *         'driver' => 'monolog',
 *         'handler' => Monolog\Handler\StreamHandler::class,
 *         'with' => ['stream' => 'php://stderr'],
 *         'formatter' => App\Logging\TokioPhpFormatter::class,
 *         'formatter_with' => ['service' => env('APP_NAME', 'laravel')],
 *     ],
 * ],
 *
 * ============================================================================
 * Standalone usage:
 * ============================================================================
 *
 * use Monolog\Logger;
 * use Monolog\Handler\StreamHandler;
 * use App\Logging\TokioPhpFormatter;
 *
 * $log = new Logger('myapp');
 * $handler = new StreamHandler('php://stderr', Logger::DEBUG);
 * $handler->setFormatter(new TokioPhpFormatter('myapp'));
 * $log->pushHandler($handler);
 *
 * $log->info('User logged in', ['user_id' => 123]);
 * $log->error('Query failed', ['exception' => $e, 'query' => $sql]);
 */
