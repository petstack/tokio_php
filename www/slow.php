<?php
// Slow script for testing graceful shutdown
$sleep_time = $_GET['sleep'] ?? 3;
sleep((int)$sleep_time);
echo json_encode([
    'status' => 'completed',
    'slept' => (int)$sleep_time,
    'time' => date('c')
]);
