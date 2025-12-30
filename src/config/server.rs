//! Server configuration.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use super::parse::{env_opt, env_or, parse_duration};
use super::ConfigError;

/// Static file cache TTL configuration.
#[derive(Clone, Debug)]
pub struct StaticCacheTtl(pub Option<Duration>);

impl StaticCacheTtl {
    /// Parse duration string (e.g., "1d", "1w", "off").
    pub fn parse(s: &str) -> Self {
        match parse_duration(s) {
            Ok(d) => Self(d),
            Err(_) => Self::default(),
        }
    }

    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.0.is_some()
    }

    #[inline]
    pub fn as_secs(&self) -> u64 {
        self.0.map(|d| d.as_secs()).unwrap_or(0)
    }

    #[inline]
    pub fn as_duration(&self) -> Option<Duration> {
        self.0
    }
}

impl Default for StaticCacheTtl {
    fn default() -> Self {
        Self(Some(Duration::from_secs(86400))) // 1 day
    }
}

/// Request timeout configuration.
#[derive(Clone, Debug)]
pub struct RequestTimeout(pub Option<Duration>);

impl RequestTimeout {
    /// Parse duration string (e.g., "30s", "2m", "off").
    pub fn parse(s: &str) -> Self {
        match parse_duration(s) {
            Ok(d) => Self(d),
            Err(_) => Self::default(),
        }
    }

    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.0.is_some()
    }

    #[inline]
    pub fn as_secs(&self) -> u64 {
        self.0.map(|d| d.as_secs()).unwrap_or(0)
    }

    #[inline]
    pub fn as_duration(&self) -> Option<Duration> {
        self.0
    }
}

impl Default for RequestTimeout {
    fn default() -> Self {
        Self(Some(Duration::from_secs(120))) // 2 minutes
    }
}

/// TLS configuration.
#[derive(Clone, Debug, Default)]
pub struct TlsConfig {
    /// Path to TLS certificate (PEM format).
    pub cert_path: Option<PathBuf>,
    /// Path to TLS private key (PEM format).
    pub key_path: Option<PathBuf>,
}

impl TlsConfig {
    /// Check if TLS is configured.
    pub fn is_enabled(&self) -> bool {
        self.cert_path.is_some() && self.key_path.is_some()
    }

    /// Load from environment variables.
    pub fn from_env() -> Self {
        Self {
            cert_path: env_opt("TLS_CERT").map(PathBuf::from),
            key_path: env_opt("TLS_KEY").map(PathBuf::from),
        }
    }
}

/// Server configuration loaded from environment.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// Listen address (default: 0.0.0.0:8080).
    pub listen_addr: SocketAddr,
    /// Document root directory (default: /var/www/html).
    pub document_root: PathBuf,
    /// Index file for single entry point mode (e.g., index.php).
    pub index_file: Option<String>,
    /// Internal server address for /health and /metrics.
    pub internal_addr: Option<SocketAddr>,
    /// Directory with custom error pages.
    pub error_pages_dir: Option<PathBuf>,
    /// Graceful shutdown drain timeout.
    pub drain_timeout: Duration,
    /// Static file cache TTL.
    pub static_cache_ttl: StaticCacheTtl,
    /// Request timeout.
    pub request_timeout: RequestTimeout,
    /// TLS configuration.
    pub tls: TlsConfig,
}

impl ServerConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        // Parse listen address
        let listen_addr: SocketAddr = env_or("LISTEN_ADDR", "0.0.0.0:8080")
            .parse()
            .map_err(|e| ConfigError::Parse {
                key: "LISTEN_ADDR".into(),
                value: env_or("LISTEN_ADDR", "0.0.0.0:8080"),
                error: format!("{}", e),
            })?;

        // Parse internal address
        let internal_addr = env_opt("INTERNAL_ADDR")
            .map(|s| {
                s.parse::<SocketAddr>().map_err(|e| ConfigError::Parse {
                    key: "INTERNAL_ADDR".into(),
                    value: s,
                    error: format!("{}", e),
                })
            })
            .transpose()?;

        // Parse drain timeout
        let drain_timeout_secs: u64 = env_or("DRAIN_TIMEOUT_SECS", "30")
            .parse()
            .map_err(|e| ConfigError::Parse {
                key: "DRAIN_TIMEOUT_SECS".into(),
                value: env_or("DRAIN_TIMEOUT_SECS", "30"),
                error: format!("{}", e),
            })?;

        Ok(Self {
            listen_addr,
            document_root: PathBuf::from(env_or("DOCUMENT_ROOT", "/var/www/html")),
            index_file: env_opt("INDEX_FILE"),
            internal_addr,
            error_pages_dir: env_opt("ERROR_PAGES_DIR").map(PathBuf::from),
            drain_timeout: Duration::from_secs(drain_timeout_secs),
            static_cache_ttl: StaticCacheTtl::parse(&env_or("STATIC_CACHE_TTL", "1d")),
            request_timeout: RequestTimeout::parse(&env_or("REQUEST_TIMEOUT", "2m")),
            tls: TlsConfig::from_env(),
        })
    }
}
