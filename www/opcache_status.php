<?php

header('Content-Type: application/json');

$status = opcache_get_status(true);

if (!$status) {
    echo json_encode(['error' => 'OPcache not enabled']);
    exit;
}

$stats = $status['opcache_statistics'];
$memory = $status['memory_usage'];

$result = [
    'enabled' => $status['opcache_enabled'],
    'cache_full' => $status['cache_full'],

    'statistics' => [
        'cached_scripts' => $stats['num_cached_scripts'],
        'hits' => $stats['hits'],
        'misses' => $stats['misses'],
        'hit_rate' => $stats['hits'] > 0
            ? round($stats['hits'] / ($stats['hits'] + $stats['misses']) * 100, 2)
            : 0,
        'oom_restarts' => $stats['oom_restarts'],
        'hash_restarts' => $stats['hash_restarts'],
    ],

    'memory' => [
        'used_mb' => round($memory['used_memory'] / 1024 / 1024, 2),
        'free_mb' => round($memory['free_memory'] / 1024 / 1024, 2),
        'wasted_mb' => round($memory['wasted_memory'] / 1024 / 1024, 2),
        'wasted_percent' => round($memory['current_wasted_percentage'], 2),
    ],

    'interned_strings' => [
        'used_mb' => round($status['interned_strings_usage']['used_memory'] / 1024 / 1024, 2),
        'free_mb' => round($status['interned_strings_usage']['free_memory'] / 1024 / 1024, 2),
        'count' => $status['interned_strings_usage']['number_of_strings'],
    ],
];

// JIT info
if (isset($status['jit'])) {
    $jit = $status['jit'];
    $result['jit'] = [
        'enabled' => $jit['enabled'],
        'on' => $jit['on'],
        'kind' => $jit['kind'],
        'buffer_size_mb' => round($jit['buffer_size'] / 1024 / 1024, 2),
        'buffer_used_mb' => round($jit['buffer_used'] / 1024 / 1024, 2),
        'buffer_free_mb' => round($jit['buffer_free'] / 1024 / 1024, 2),
    ];
}

// Top 10 scripts by hits
if (isset($status['scripts'])) {
    $scripts = $status['scripts'];
    usort($scripts, fn($a, $b) => $b['hits'] - $a['hits']);
    $result['top_scripts'] = array_map(
        fn($s) => [
            'path' => basename($s['full_path']),
            'hits' => $s['hits'],
            'memory_kb' => round($s['memory_consumption'] / 1024, 2),
        ],
        array_slice($scripts, 0, 10)
    );
}

echo json_encode($result, JSON_PRETTY_PRINT);
