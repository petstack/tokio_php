//! Server configuration.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use super::parse::{env_opt, env_or, parse_duration};
use super::ConfigError;

// Default values as constants
const DEFAULT_STATIC_CACHE_TTL_SECS: u64 = 86400; // 1 day
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 120; // 2 minutes
const DEFAULT_DRAIN_TIMEOUT_SECS: u64 = 30;

/// Duration-based configuration that can be disabled.
///
/// Pre-computes seconds for zero-cost access at runtime.
/// Uses 0 to represent "disabled" state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptionalDuration {
    secs: u64,
}

impl OptionalDuration {
    /// Disabled duration (0 seconds).
    pub const DISABLED: Self = Self { secs: 0 };

    /// Create from seconds (0 = disabled).
    #[inline]
    pub const fn from_secs(secs: u64) -> Self {
        Self { secs }
    }

    /// Parse from string (e.g., "30s", "2m", "off").
    /// Falls back to default_secs on parse error.
    pub fn parse(s: &str, default_secs: u64) -> Self {
        match parse_duration(s) {
            Ok(Some(d)) => Self::from_secs(d.as_secs()),
            Ok(None) => Self::DISABLED,
            Err(_) => Self::from_secs(default_secs),
        }
    }

    /// Check if duration is enabled (non-zero).
    #[inline]
    pub const fn is_enabled(&self) -> bool {
        self.secs > 0
    }

    /// Get duration in seconds (0 if disabled).
    #[inline]
    pub const fn as_secs(&self) -> u64 {
        self.secs
    }

    /// Convert to Option<Duration>.
    #[inline]
    pub const fn as_duration(&self) -> Option<Duration> {
        if self.secs > 0 {
            Some(Duration::from_secs(self.secs))
        } else {
            None
        }
    }
}

impl Default for OptionalDuration {
    fn default() -> Self {
        Self::DISABLED
    }
}

/// Static file cache TTL (default: 1 day).
pub type StaticCacheTtl = OptionalDuration;

/// Request timeout (default: 2 minutes).
pub type RequestTimeout = OptionalDuration;

/// TLS configuration.
#[derive(Clone, Debug)]
pub struct TlsConfig {
    /// Path to TLS certificate (PEM format).
    pub cert_path: Option<PathBuf>,
    /// Path to TLS private key (PEM format).
    pub key_path: Option<PathBuf>,
    /// Pre-computed enabled flag (zero-cost check).
    enabled: bool,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            cert_path: None,
            key_path: None,
            enabled: false,
        }
    }
}

impl TlsConfig {
    /// Check if TLS is configured (pre-computed, zero-cost).
    #[inline]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Load from environment variables.
    pub fn from_env() -> Self {
        let cert_path = env_opt("TLS_CERT").map(PathBuf::from);
        let key_path = env_opt("TLS_KEY").map(PathBuf::from);
        let enabled = cert_path.is_some() && key_path.is_some();
        Self {
            cert_path,
            key_path,
            enabled,
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
        Ok(Self {
            listen_addr: Self::parse_addr("LISTEN_ADDR", "0.0.0.0:8080")?,
            document_root: PathBuf::from(env_or("DOCUMENT_ROOT", "/var/www/html")),
            index_file: env_opt("INDEX_FILE"),
            internal_addr: Self::parse_addr_opt("INTERNAL_ADDR")?,
            error_pages_dir: env_opt("ERROR_PAGES_DIR").map(PathBuf::from),
            drain_timeout: Duration::from_secs(Self::parse_u64(
                "DRAIN_TIMEOUT_SECS",
                DEFAULT_DRAIN_TIMEOUT_SECS,
            )?),
            static_cache_ttl: OptionalDuration::parse(
                &env_or("STATIC_CACHE_TTL", "1d"),
                DEFAULT_STATIC_CACHE_TTL_SECS,
            ),
            request_timeout: OptionalDuration::parse(
                &env_or("REQUEST_TIMEOUT", "2m"),
                DEFAULT_REQUEST_TIMEOUT_SECS,
            ),
            tls: TlsConfig::from_env(),
        })
    }

    fn parse_addr(key: &str, default: &str) -> Result<SocketAddr, ConfigError> {
        let raw = env_or(key, default);
        raw.parse().map_err(|e| ConfigError::Parse {
            key: key.into(),
            value: raw,
            error: format!("{e}"),
        })
    }

    fn parse_addr_opt(key: &str) -> Result<Option<SocketAddr>, ConfigError> {
        env_opt(key)
            .map(|raw| {
                raw.parse().map_err(|e| ConfigError::Parse {
                    key: key.into(),
                    value: raw,
                    error: format!("{e}"),
                })
            })
            .transpose()
    }

    fn parse_u64(key: &str, default: u64) -> Result<u64, ConfigError> {
        let raw = env_or(key, &default.to_string());
        raw.parse().map_err(|e| ConfigError::Parse {
            key: key.into(),
            value: raw,
            error: format!("{e}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // OptionalDuration tests
    #[test]
    fn test_optional_duration_disabled() {
        let d = OptionalDuration::DISABLED;
        assert!(!d.is_enabled());
        assert_eq!(d.as_secs(), 0);
        assert!(d.as_duration().is_none());
    }

    #[test]
    fn test_optional_duration_enabled() {
        let d = OptionalDuration::from_secs(3600);
        assert!(d.is_enabled());
        assert_eq!(d.as_secs(), 3600);
        assert_eq!(d.as_duration(), Some(Duration::from_secs(3600)));
    }

    #[test]
    fn test_optional_duration_parse_day() {
        let d = OptionalDuration::parse("1d", 0);
        assert!(d.is_enabled());
        assert_eq!(d.as_secs(), 86400);
    }

    #[test]
    fn test_optional_duration_parse_week() {
        let d = OptionalDuration::parse("1w", 0);
        assert!(d.is_enabled());
        assert_eq!(d.as_secs(), 604800);
    }

    #[test]
    fn test_optional_duration_parse_minutes() {
        let d = OptionalDuration::parse("30m", 0);
        assert!(d.is_enabled());
        assert_eq!(d.as_secs(), 1800);
    }

    #[test]
    fn test_optional_duration_parse_year() {
        let d = OptionalDuration::parse("1y", 0);
        assert!(d.is_enabled());
        assert_eq!(d.as_secs(), 31536000);
    }

    #[test]
    fn test_optional_duration_parse_off() {
        let d = OptionalDuration::parse("off", 86400);
        assert!(!d.is_enabled());
        assert_eq!(d.as_secs(), 0);
    }

    #[test]
    fn test_optional_duration_parse_zero() {
        let d = OptionalDuration::parse("0", 86400);
        assert!(!d.is_enabled());
        assert_eq!(d.as_secs(), 0);
    }

    #[test]
    fn test_optional_duration_parse_invalid_fallback() {
        let d = OptionalDuration::parse("invalid", 86400);
        assert!(d.is_enabled());
        assert_eq!(d.as_secs(), 86400);
    }

    #[test]
    fn test_optional_duration_parse_seconds() {
        let d = OptionalDuration::parse("30s", 0);
        assert!(d.is_enabled());
        assert_eq!(d.as_secs(), 30);
    }

    #[test]
    fn test_optional_duration_parse_hours() {
        let d = OptionalDuration::parse("1h", 0);
        assert!(d.is_enabled());
        assert_eq!(d.as_secs(), 3600);
    }

    // TlsConfig tests
    #[test]
    fn test_tls_config_disabled_by_default() {
        let tls = TlsConfig::default();
        assert!(!tls.is_enabled());
        assert!(tls.cert_path.is_none());
        assert!(tls.key_path.is_none());
    }

    #[test]
    fn test_tls_config_enabled_when_both_paths_set() {
        let tls = TlsConfig {
            cert_path: Some(PathBuf::from("/path/to/cert.pem")),
            key_path: Some(PathBuf::from("/path/to/key.pem")),
            enabled: true,
        };
        assert!(tls.is_enabled());
    }

    #[test]
    fn test_tls_config_disabled_when_only_cert() {
        let tls = TlsConfig {
            cert_path: Some(PathBuf::from("/path/to/cert.pem")),
            key_path: None,
            enabled: false,
        };
        assert!(!tls.is_enabled());
    }

    #[test]
    fn test_tls_config_disabled_when_only_key() {
        let tls = TlsConfig {
            cert_path: None,
            key_path: Some(PathBuf::from("/path/to/key.pem")),
            enabled: false,
        };
        assert!(!tls.is_enabled());
    }
}
