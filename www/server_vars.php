<?php
header('Content-Type: text/plain');

$vars = [
    'REQUEST_TIME',
    'REQUEST_TIME_FLOAT',
    'SERVER_NAME',
    'SERVER_ADDR',
    'SERVER_PORT',
    'DOCUMENT_ROOT',
    'SCRIPT_NAME',
    'SCRIPT_FILENAME',
    'PHP_SELF',
    'PATH_INFO',
    'HTTP_USER_AGENT',
    'HTTP_REFERER',
    'HTTP_ACCEPT_LANGUAGE',
    'HTTP_HOST',
    'HTTP_ACCEPT',
    'HTTP_COOKIE',
    'HTTPS',
    'SSL_PROTOCOL',
    'REQUEST_METHOD',
    'REQUEST_URI',
    'QUERY_STRING',
    'REMOTE_ADDR',
    'REMOTE_PORT',
    'SERVER_SOFTWARE',
    'SERVER_PROTOCOL',
    'CONTENT_TYPE',
    'GATEWAY_INTERFACE',
];

foreach ($vars as $var) {
    $value = $_SERVER[$var] ?? '(not set)';
    printf("%-22s = %s\n", $var, $value);
}
