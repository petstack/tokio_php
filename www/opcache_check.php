<?php
header('Content-Type: application/json');
$status = opcache_get_status(true);
$result = [
    'sapi' => php_sapi_name(),
    'opcache_enabled' => $status ? $status['opcache_enabled'] : false,
    'memory_consumption_ini' => ini_get('opcache.memory_consumption'),
];
if ($status && isset($status['memory_usage'])) {
    $result['memory'] = $status['memory_usage'];
}
if ($status && isset($status['opcache_statistics'])) {
    $result['statistics'] = $status['opcache_statistics'];
}
if ($status && isset($status['scripts'])) {
    $result['scripts_count'] = count($status['scripts']);
    $result['scripts'] = array_keys($status['scripts']);
}
echo json_encode($result, JSON_PRETTY_PRINT);
