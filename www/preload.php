<?php

/**
 * OPcache Preload Script
 *
 * This script runs once at server startup and preloads PHP files into OPcache.
 * Preloaded classes and functions are available to all requests without
 * compilation overhead.
 *
 * Usage: opcache.preload=/var/www/html/preload.php
 *
 * Benefits:
 * - Eliminates compilation time for preloaded files
 * - Reduces memory usage (shared across all workers)
 * - Classes are "linked" at startup (faster autoloading)
 */

// Preload configuration
$preloadPaths = [
    // Add framework paths here
    // __DIR__ . '/vendor/autoload.php',
    // __DIR__ . '/src/',
];

$preloadFiles = [
    // Specific files to preload
    // __DIR__ . '/app/Kernel.php',
    // __DIR__ . '/config/services.php',
];

// Statistics
$stats = [
    'files' => 0,
    'classes' => count(get_declared_classes()),
    'functions' => count(get_defined_functions()['user']),
];

/**
 * Recursively preload PHP files from directory
 */
function preloadDirectory(string $path): int
{
    $count = 0;

    if (!is_dir($path)) {
        return $count;
    }

    $iterator = new RecursiveIteratorIterator(
        new RecursiveDirectoryIterator($path, RecursiveDirectoryIterator::SKIP_DOTS),
        RecursiveIteratorIterator::SELF_FIRST
    );

    foreach ($iterator as $file) {
        if ($file->isFile() && $file->getExtension() === 'php') {
            if (preloadFile($file->getPathname())) {
                $count++;
            }
        }
    }

    return $count;
}

/**
 * Preload single PHP file
 */
function preloadFile(string $path): bool
{
    if (!file_exists($path)) {
        return false;
    }

    try {
        // opcache_compile_file compiles without executing
        if (function_exists('opcache_compile_file')) {
            return opcache_compile_file($path);
        }

        // Fallback: require (executes the file)
        require_once $path;
        return true;
    } catch (Throwable $e) {
        // Log errors but continue preloading
        error_log("Preload error [{$path}]: " . $e->getMessage());
        return false;
    }
}

// Preload directories
foreach ($preloadPaths as $path) {
    $stats['files'] += preloadDirectory($path);
}

// Preload individual files
foreach ($preloadFiles as $file) {
    if (preloadFile($file)) {
        $stats['files']++;
    }
}

// Calculate new classes/functions
$stats['new_classes'] = count(get_declared_classes()) - $stats['classes'];
$stats['new_functions'] = count(get_defined_functions()['user']) - $stats['functions'];

// Log preload results in unified JSON format
$logEntry = json_encode([
    'ts' => gmdate('Y-m-d\TH:i:s.') . sprintf('%03d', (int)(microtime(true) * 1000) % 1000) . 'Z',
    'level' => 'info',
    'type' => 'app',
    'msg' => sprintf('Preload complete: %d files, %d classes, %d functions',
        $stats['files'], $stats['new_classes'], $stats['new_functions']),
    'ctx' => ['service' => 'tokio_php'],
    'data' => [
        'files' => $stats['files'],
        'classes' => $stats['new_classes'],
        'functions' => $stats['new_functions'],
    ],
], JSON_UNESCAPED_SLASHES);

// Output to stderr (captured by Docker)
file_put_contents('php://stderr', $logEntry . "\n");
