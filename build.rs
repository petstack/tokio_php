use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Only link PHP when the "php" feature is enabled
    if env::var("CARGO_FEATURE_PHP").is_err() {
        return;
    }

    // Try to find PHP config
    let php_config = env::var("PHP_CONFIG").unwrap_or_else(|_| "php-config84".to_string());

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

    // Add common paths including php84 specific path
    println!("cargo:rustc-link-search=native=/usr/lib");
    println!("cargo:rustc-link-search=native=/usr/lib/php84");

    // Link against php embed library
    println!("cargo:rustc-link-lib=dylib=php");

    // Parse additional libraries from --libs
    for flag in libs.split_whitespace() {
        if let Some(lib) = flag.strip_prefix("-l") {
            if lib != "php" {
                println!("cargo:rustc-link-lib={}", lib);
            }
        }
    }
}
