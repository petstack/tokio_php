use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.git/HEAD");

    // Get git commit hash (8 characters)
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short=8", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=GIT_HASH={}", git_hash);

    // Check if working directory is dirty
    let is_dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    let build_version = if is_dirty {
        format!("{}-dirty", git_hash)
    } else {
        git_hash
    };

    println!("cargo:rustc-env=BUILD_VERSION={}", build_version);

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
    println!("cargo:rustc-link-search=native=/usr/lib/php84");
    println!("cargo:rustc-link-search=native=/usr/local/lib"); // Official PHP images

    // Link against php embed library
    println!("cargo:rustc-link-lib=dylib=php");

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
