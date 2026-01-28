use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Set empty build version (git hash not available in Docker builds)
    println!("cargo:rustc-env=BUILD_VERSION=");

    // Only link PHP when the "php" feature is enabled
    if env::var("CARGO_FEATURE_PHP").is_err() {
        return;
    }

    // Try to find PHP config (php-config for official images, php-config84 for Alpine)
    let php_config = env::var("PHP_CONFIG").unwrap_or_else(|_| {
        // Try php-config first (official PHP images), then php-config84 (Alpine)
        if Command::new("php-config").arg("--version").output().is_ok() {
            "php-config".to_string()
        } else {
            "php-config84".to_string()
        }
    });

    // Get PHP library directory using --ldflags
    let ldflags = Command::new(&php_config)
        .arg("--ldflags")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    // Get PHP libs
    let libs = Command::new(&php_config)
        .arg("--libs")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    // Parse -L flags from ldflags
    for flag in ldflags.split_whitespace() {
        if let Some(path) = flag.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={}", path);
        }
    }

    // Add common paths for different PHP installations
    println!("cargo:rustc-link-search=native=/usr/lib");
    println!("cargo:rustc-link-search=native=/lib");
    println!("cargo:rustc-link-search=native=/usr/lib/php84");
    println!("cargo:rustc-link-search=native=/usr/local/lib"); // Official PHP images

    // Debug: Print library search paths
    eprintln!(
        "build.rs: Adding library search paths: /usr/lib, /lib, /usr/lib/php84, /usr/local/lib"
    );

    // Link against php embed library
    println!("cargo:rustc-link-lib=dylib=php");

    // Link against tokio_bridge shared library (required for Rust <-> PHP communication)
    // This provides shared TLS context for finish_request, heartbeat, etc.
    println!("cargo:rustc-link-lib=dylib=tokio_bridge");

    // Link against tokio_sapi static library (if available)
    // This provides the extension functions for direct superglobal access
    println!("cargo:rustc-link-lib=static=tokio_sapi");

    // Parse additional libraries from --libs
    for flag in libs.split_whitespace() {
        if let Some(lib) = flag.strip_prefix("-l") {
            if lib != "php" {
                println!("cargo:rustc-link-lib={}", lib);
            }
        }
    }
}
