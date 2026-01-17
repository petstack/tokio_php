<?php
// SSE debug test - output to stderr for debugging
error_reporting(E_ALL);

function debug($msg) {
    file_put_contents('php://stderr', "[PHP DEBUG] $msg\n");
}

debug("Script started");
debug("Output buffering level: " . ob_get_level());

debug("echo chunk1");
echo "data: chunk1\n\n";
debug("Output buffering level after echo: " . ob_get_level());

debug("Calling flush()");
flush();
debug("After flush(), ob level: " . ob_get_level());

debug("echo chunk2");
echo "data: chunk2\n\n";
debug("Output buffering level after echo2: " . ob_get_level());

debug("Calling flush() #2");
flush();
debug("After flush() #2, ob level: " . ob_get_level());

debug("echo chunk3");
echo "data: chunk3\n\n";
debug("Output buffering level after echo3: " . ob_get_level());

debug("Calling flush() #3");
flush();
debug("Script done");
