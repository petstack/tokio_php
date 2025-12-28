<?php
function _renderSuperglobalTabs(): void {
    $globals = [
        'get' => ['$_GET', $_GET],
        'post' => ['$_POST', $_POST],
        'server' => ['$_SERVER', $_SERVER],
        'cookie' => ['$_COOKIE', $_COOKIE],
        'files' => ['$_FILES', $_FILES],
        'request' => ['$_REQUEST', $_REQUEST],
    ];

    foreach ($globals as $id => [$title, $data]) {
        $count = count($data);
        echo "<div class=\"tab-content\" id=\"tab-{$id}\">";
        echo "<div class=\"tab-header\">{$title} ({$count} " . ($count === 1 ? 'item' : 'items') . ")</div>";
        echo "<div class=\"tab-body\">";
        if (empty($data)) {
            echo "<div class=\"tab-empty\">Empty</div>";
        } else {
            echo "<table class=\"kv-table\">";
            foreach ($data as $key => $value) {
                $key = htmlspecialchars((string)$key);
                if (is_array($value)) {
                    $displayValue = htmlspecialchars(json_encode($value, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES));
                    $class = 'val-array';
                } elseif (is_numeric($value)) {
                    $displayValue = htmlspecialchars((string)$value);
                    $class = 'val-number';
                } else {
                    $displayValue = htmlspecialchars((string)$value);
                    $class = 'val-string';
                }
                echo "<tr><td class=\"key\">{$key}</td><td class=\"val {$class}\">{$displayValue}</td></tr>";
            }
            echo "</table>";
        }
        echo "</div></div>";
    }
}
_renderSuperglobalTabs();
