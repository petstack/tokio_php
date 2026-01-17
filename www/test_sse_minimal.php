<?php
// Minimal SSE test - output streaming check
// If chunks come one at a time, streaming works

echo "data: chunk1\n\n";
flush();

echo "data: chunk2\n\n";
flush();

echo "data: chunk3\n\n";
flush();

// Also output some debug info
error_log("SSE test: tokio_is_streaming = " . (function_exists('tokio_is_streaming') ? (tokio_is_streaming() ? "true" : "false") : "function not found"));
