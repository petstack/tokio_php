<?php
header('Content-Type: text/plain');
echo "=== get_loaded_extensions() ===\n";
$exts = get_loaded_extensions();
sort($exts);
echo implode("\n", $exts);
echo "\n\n=== Zend extensions ===\n";
$exts = get_loaded_extensions(true);
sort($exts);
echo implode("\n", $exts);
