<?php
// Test: tokio_early_hints() function
// Note: Full streaming support is infrastructure-ready but pending server handler changes.
// Currently returns false (callback not set).

$result = tokio_early_hints([
    'Link: </style.css>; rel=preload; as=style',
    'Link: </app.js>; rel=preload; as=script',
]);

echo "tokio_early_hints() result: " . ($result ? 'true' : 'false') . "\n";
echo "Function exists: " . (function_exists('tokio_early_hints') ? 'yes' : 'no') . "\n";

// Note: Returns false because the server handler doesn't set the callback yet.
// Once full early hints support is implemented:
// 1. Server will use tokio::select! to listen for hints while script runs
// 2. PHP sends hints via bridge callback
// 3. Server immediately sends HTTP 103 response
// 4. Browser starts preloading while PHP continues working
