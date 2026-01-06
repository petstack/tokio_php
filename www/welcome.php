<?php
$status = opcache_get_status(false);
$enabled = $status && $status['opcache_enabled'];
?>
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <meta http-equiv="X-UA-Compatible" content="ie=edge"/>
    <title>Welcome to tokio_php</title>

    <!-- Tabler CSS -->
    <link href="https://cdn.jsdelivr.net/npm/@tabler/core@1.4.0/dist/css/tabler.min.css" rel="stylesheet" />
    <link href="https://cdn.jsdelivr.net/npm/@tabler/icons-webfont@2.44.0/tabler-icons.min.css" rel="stylesheet" />

    <style>
      :root {
        --tblr-font-sans-serif: -apple-system, BlinkMacSystemFont, San Francisco, Segoe UI, Roboto, Helvetica Neue, sans-serif;
        --rust-orange: #B7472A;
        --php-purple: #777BB4;
        --bg-light: #FFFFFF;
        --bg-subtle: #F6F8FA;
        --bg-muted: #F0F2F4;
        --text-primary: #24292F;
        --text-secondary: #57606A;
        --border-color: #D0D7DE;
      }

      body {
        background: var(--bg-subtle);
        color: var(--text-primary);
        min-height: 100vh;
      }

      .hero {
        background: linear-gradient(135deg, var(--bg-light) 0%, var(--bg-muted) 100%);
        padding: 3rem 0;
        text-align: center;
      }

      .logo {
        font-size: 3rem;
        font-weight: 800;
        margin-bottom: 1rem;
      }

      .logo-rust { color: var(--rust-orange); }
      .logo-php { color: var(--php-purple); }

      .tagline {
        font-size: 1.25rem;
        color: var(--text-secondary);
      }

      .feature-card {
        background: var(--bg-light);
        border: 1px solid var(--border-color);
        border-radius: 12px;
        padding: 1.5rem;
        height: 100%;
        transition: all 0.2s ease;
      }

      .feature-card:hover {
        border-color: var(--rust-orange);
        transform: translateY(-2px);
        box-shadow: 0 4px 12px rgba(0,0,0,0.08);
      }

      .feature-icon {
        width: 48px;
        height: 48px;
        border-radius: 10px;
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: 24px;
        margin-bottom: 1rem;
      }

      .section-title {
        font-size: 1.75rem;
        font-weight: 700;
        margin-bottom: 2rem;
        text-align: center;
      }

      .footer {
        background: var(--bg-light);
        border-top: 1px solid var(--border-color);
        padding: 2rem 0;
        text-align: center;
      }

      .features-section {
        padding: 3rem 0;
      }
    </style>
  </head>
  <body>
    <!-- Hero -->
    <section class="hero">
      <div class="container">
        <div class="logo">
          <span class="logo-rust">tokio</span><span class="logo-php">_php</span>
        </div>
        <p class="tagline mb-3">Async PHP Server powered by Rust & Tokio</p>
      </div>
    </section>

    <!-- Features -->
    <section class="features-section">
      <div class="container">
        <h2 class="section-title">Features</h2>
        <div class="row g-3">
          <!-- PHP 8.4 & 8.5 -->
          <div class="col-6 col-lg-3 col-xl-3">
            <div class="feature-card text-center">
              <div class="feature-icon bg-purple-lt text-purple mx-auto">
                <i class="ti ti-brand-php"></i>
              </div>
              <h5>PHP 8.4 & 8.5</h5>
              <p class="text-muted small mb-0">Requires ZTS + embed SAPI (<abbr title="Bring Your Own PHP">BYOP</abbr>)</p>
            </div>
          </div>

          <!-- HTTP/1 & HTTP/2 & TLS 1.3 -->
          <div class="col-6 col-lg-4 col-xl-3">
            <div class="feature-card text-center">
              <div class="feature-icon bg-blue-lt text-blue mx-auto">
                <i class="ti ti-lock"></i>
              </div>
              <h5>HTTP/1 & HTTP/2 & TLS 1.3</h5>
              <p class="text-muted small mb-0">Modern protocols with ALPN negotiation</p>
            </div>
          </div>

          <!-- OPcache & JIT -->
          <div class="col-6 col-lg-4 col-xl-3">
            <div class="feature-card text-center">
              <div class="feature-icon bg-yellow-lt text-yellow mx-auto">
                <i class="ti ti-bolt"></i>
              </div>
              <h5>OPcache & JIT</h5>
              <p class="text-muted small mb-0">Full support with preloading</p>
            </div>
          </div>

          <!-- Worker Pool -->
          <div class="col-6 col-lg-4 col-xl-3">
            <div class="feature-card text-center">
              <div class="feature-icon bg-orange-lt text-orange mx-auto">
                <i class="ti ti-users"></i>
              </div>
              <h5>Worker Pool</h5>
              <p class="text-muted small mb-0">Multi-threaded PHP execution</p>
            </div>
          </div>

          <!-- Compression -->
          <div class="col-6 col-lg-6 col-xl-3">
            <div class="feature-card text-center">
              <div class="feature-icon bg-cyan-lt text-cyan mx-auto">
                <i class="ti ti-file-zip"></i>
              </div>
              <h5>Compression</h5>
              <p class="text-muted small mb-0">Automatic Brotli for responses</p>
            </div>
          </div>

          <!-- Static Caching -->
          <div class="col-6 col-lg-6 col-xl-3">
            <div class="feature-card text-center">
              <div class="feature-icon bg-green-lt text-green mx-auto">
                <i class="ti ti-database"></i>
              </div>
              <h5>Static Caching</h5>
              <p class="text-muted small mb-0">In-memory cache for static files</p>
            </div>
          </div>

          <!-- Single Entry Point -->
          <div class="col-6 col-lg-4 col-xl-3">
            <div class="feature-card text-center">
              <div class="feature-icon bg-purple-lt text-purple mx-auto">
                <i class="ti ti-file-code"></i>
              </div>
              <h5>Single Entry Point</h5>
              <p class="text-muted small mb-0">Laravel & Symfony routing</p>
            </div>
          </div>

          <!-- Rate Limiting -->
          <div class="col-6 col-lg-4 col-xl-3">
            <div class="feature-card text-center">
              <div class="feature-icon bg-red-lt text-red mx-auto">
                <i class="ti ti-shield-check"></i>
              </div>
              <h5>Rate Limiting</h5>
              <p class="text-muted small mb-0">Per-IP request throttling</p>
            </div>
          </div>

          <!-- Request Heartbeat -->
          <div class="col-6 col-lg-4 col-xl-3">
            <div class="feature-card text-center">
              <div class="feature-icon bg-pink-lt text-pink mx-auto">
                <i class="ti ti-heartbeat"></i>
              </div>
              <h5>Request Heartbeat</h5>
              <p class="text-muted small mb-0">Extend timeout for long tasks</p>
            </div>
          </div>

          <!-- Error Pages -->
          <div class="col-6 col-lg-6 col-xl-3">
            <div class="feature-card text-center">
              <div class="feature-icon bg-azure-lt text-azure mx-auto">
                <i class="ti ti-alert-triangle"></i>
              </div>
              <h5>Error Pages</h5>
              <p class="text-muted small mb-0">Custom HTML for 4xx/5xx errors</p>
            </div>
          </div>

          <!-- Internal Server -->
          <div class="col-6 col-lg-6 col-xl-3">
            <div class="feature-card text-center">
              <div class="feature-icon bg-indigo-lt text-indigo mx-auto">
                <i class="ti ti-activity"></i>
              </div>
              <h5>Internal Server</h5>
              <p class="text-muted small mb-0">Health, metrics & config endpoints</p>
            </div>
          </div>

          <!-- Documentation -->
          <div class="col-6 col-lg-6 col-xl-3">
            <a href="https://github.com/petstack/tokio_php/tree/master/docs" target="_blank" class="text-decoration-none">
              <div class="feature-card text-center">
                <div class="feature-icon bg-dark-lt text-dark mx-auto">
                  <i class="ti ti-book"></i>
                </div>
                <h5>Documentation</h5>
                <p class="text-muted small mb-0">Configuration, examples & guides</p>
              </div>
            </a>
          </div>
        </div>
      </div>
    </section>

    <!-- Footer -->
    <footer class="footer">
      <div class="container">
        <div class="d-flex justify-content-center gap-3">
          <a href="https://github.com/petstack/tokio_php" class="btn btn-outline-secondary" target="_blank">
            <i class="ti ti-brand-github me-1"></i> GitHub
          </a>
          <a href="https://hub.docker.com/r/diolektor/tokio_php" class="btn btn-outline-secondary" target="_blank">
            <i class="ti ti-brand-docker me-1"></i> Docker Hub
          </a>
        </div>
      </div>
      <div style="margin-top: 32px; padding-top: 16px; border-top: 1px solid #eee; font-size: 12px; color: #999;">
          <?= number_format((microtime(true) - $_SERVER['REQUEST_TIME_FLOAT']) * 1000, 2) ?> ms (OPcache: <?= $enabled ? 'enabled' : 'disabled' ?>)
      </div>
    </footer>
  </body>
</html>
