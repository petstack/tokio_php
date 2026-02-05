<?php
header('Content-Type: text/plain');
$c = opcache_get_configuration();
print_r($c['directives']);
