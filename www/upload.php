<?php
$message = '';
$uploaded_file_info = null;

if ($_SERVER['REQUEST_METHOD'] === 'POST') {
    if (isset($_FILES['file']) && $_FILES['file']['error'] === 0) {
        $uploaded_file_info = $_FILES['file'];

        // Read first 100 bytes of file content for preview
        $preview = '';
        if (file_exists($_FILES['file']['tmp_name'])) {
            $content = file_get_contents($_FILES['file']['tmp_name'], false, null, 0, 100);
            // Check if binary
            if (preg_match('/[\x00-\x08\x0B\x0C\x0E-\x1F]/', $content)) {
                $preview = '[Binary file - ' . strlen(file_get_contents($_FILES['file']['tmp_name'])) . ' bytes]';
            } else {
                $preview = htmlspecialchars($content);
                if (strlen(file_get_contents($_FILES['file']['tmp_name'])) > 100) {
                    $preview .= '...';
                }
            }
        }
        $uploaded_file_info['preview'] = $preview;

        $message = 'File uploaded successfully!';
    } elseif (isset($_FILES['file'])) {
        $error_messages = [
            1 => 'File exceeds upload_max_filesize',
            2 => 'File exceeds MAX_FILE_SIZE',
            3 => 'File was only partially uploaded',
            4 => 'No file was uploaded',
            6 => 'Missing a temporary folder',
            7 => 'Failed to write file to disk',
            8 => 'A PHP extension stopped the upload',
        ];
        $error_code = $_FILES['file']['error'];
        $message = 'Upload error: ' . ($error_messages[$error_code] ?? "Unknown error ($error_code)");
    }
}

// Get form field value if submitted
$description = htmlspecialchars($_POST['description'] ?? '');
?>
<!DOCTYPE html>
<html>
<head>
    <title>File Upload Test - tokio_php</title>
    <style>
        body { font-family: sans-serif; margin: 50px auto; max-width: 700px; background: #1a1a2e; color: #eee; padding: 20px; }
        h1 { color: #00d9ff; }
        .info { background: #16213e; padding: 15px; border-radius: 8px; margin: 20px 0; }
        .info h3 { color: #e94560; margin-top: 0; }
        pre { background: #0f0f23; padding: 10px; border-radius: 4px; white-space: pre-wrap; word-wrap: break-word; overflow-x: auto; }
        a { color: #00d9ff; }
        .form-group { margin: 15px 0; }
        label { display: block; margin-bottom: 5px; color: #8892bf; }
        input[type="file"], input[type="text"] {
            width: 100%; padding: 10px; border: 1px solid #4a4a6a;
            background: #16213e; color: #eee; border-radius: 4px;
            box-sizing: border-box;
        }
        button {
            background: #e94560; color: white; padding: 10px 20px;
            border: none; border-radius: 4px; cursor: pointer; font-size: 16px;
        }
        button:hover { background: #c73e54; }
        .success { background: #2d5a3d; border-left: 4px solid #4caf50; padding: 15px; margin: 15px 0; }
        .error { background: #5a2d2d; border-left: 4px solid #f44336; padding: 15px; margin: 15px 0; }
        code { background: #0f0f23; padding: 2px 6px; border-radius: 4px; }
        table { width: 100%; border-collapse: collapse; margin: 10px 0; }
        td, th { padding: 8px; text-align: left; border-bottom: 1px solid #4a4a6a; }
        th { color: #8892bf; }
    </style>
</head>
<body>
    <h1>File Upload Test</h1>
    <p>Test <code>$_FILES</code> support in tokio_php</p>

    <?php if ($message): ?>
    <div class="<?= strpos($message, 'error') !== false ? 'error' : 'success' ?>">
        <strong><?= $message ?></strong>
    </div>
    <?php endif; ?>

    <?php if ($uploaded_file_info): ?>
    <div class="info">
        <h3>Uploaded File Details</h3>
        <table>
            <tr><th>Property</th><th>Value</th></tr>
            <tr><td>Original Name</td><td><code><?= htmlspecialchars($uploaded_file_info['name']) ?></code></td></tr>
            <tr><td>MIME Type</td><td><code><?= htmlspecialchars($uploaded_file_info['type']) ?></code></td></tr>
            <tr><td>Temp Path</td><td><code><?= htmlspecialchars($uploaded_file_info['tmp_name']) ?></code></td></tr>
            <tr><td>Size</td><td><code><?= number_format($uploaded_file_info['size']) ?> bytes</code></td></tr>
            <tr><td>Error Code</td><td><code><?= $uploaded_file_info['error'] ?></code> (0 = success)</td></tr>
        </table>

        <h3>Content Preview</h3>
        <pre><?= $uploaded_file_info['preview'] ?></pre>
    </div>
    <?php endif; ?>

    <div class="info">
        <h3>Upload Single File</h3>
        <form method="POST" enctype="multipart/form-data" action="/upload.php">
            <div class="form-group">
                <label for="file">Select File:</label>
                <input type="file" id="file" name="file">
            </div>
            <div class="form-group">
                <label for="description">Description (optional):</label>
                <input type="text" id="description" name="description" placeholder="Enter file description">
            </div>
            <button type="submit">Upload File</button>
        </form>
    </div>

    <div class="info">
        <h3>Upload Multiple Files</h3>
        <form method="POST" enctype="multipart/form-data" action="/upload.php">
            <div class="form-group">
                <label for="files">Select Multiple Files:</label>
                <input type="file" id="files" name="files[]" multiple>
            </div>
            <button type="submit">Upload Files</button>
        </form>
        <p style="color: #8892bf; margin-top: 10px;">Hold Ctrl/Cmd to select multiple files</p>
    </div>

    <?php if ($description): ?>
    <div class="info">
        <h3>Form Data</h3>
        <p><strong>Description:</strong> <?= $description ?></p>
    </div>
    <?php endif; ?>

    <div class="info">
        <h3>$_FILES contents:</h3>
        <pre><?= htmlspecialchars(print_r($_FILES, true)) ?></pre>

        <h3>$_POST contents:</h3>
        <pre><?= htmlspecialchars(print_r($_POST, true)) ?></pre>
    </div>

    <div class="info">
        <h3>Test with curl</h3>
        <p>Single file:</p>
        <pre>curl -F "file=@/path/to/file.txt" -F "description=Test" http://localhost:8080/upload.php</pre>
        <p>Multiple files:</p>
        <pre>curl -F "files[]=@file1.txt" -F "files[]=@file2.txt" http://localhost:8080/upload.php</pre>
    </div>

    <p><a href="/">← Back to home</a></p>
</body>
</html>
