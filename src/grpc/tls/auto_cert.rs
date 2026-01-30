//! Auto-generation of self-signed certificates for development.

use std::fs;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};

use rcgen::{
    CertificateParams, DnType, ExtendedKeyUsagePurpose, KeyPair, KeyUsagePurpose, SanType,
};
use time::{Duration, OffsetDateTime};
use tracing::{info, warn};

/// Auto-generator for self-signed certificates.
pub struct AutoCertGenerator {
    cn: String,
    validity_days: u32,
    output_dir: PathBuf,
}

impl AutoCertGenerator {
    /// Create a new certificate generator.
    pub fn new(cn: impl Into<String>, validity_days: u32, output_dir: impl Into<PathBuf>) -> Self {
        Self {
            cn: cn.into(),
            validity_days,
            output_dir: output_dir.into(),
        }
    }

    /// Ensure certificates exist and are valid.
    ///
    /// Returns paths to (certificate, private key) files.
    /// If certificates don't exist or are expiring soon, generates new ones.
    pub fn ensure_certs(&self) -> io::Result<(PathBuf, PathBuf)> {
        // Create output directory if needed
        fs::create_dir_all(&self.output_dir)?;

        let cert_path = self.output_dir.join("grpc-cert.pem");
        let key_path = self.output_dir.join("grpc-key.pem");

        // Check if certs exist and are valid
        if self.certs_valid(&cert_path, &key_path) {
            info!(
                cert = %cert_path.display(),
                "Using existing auto-generated gRPC certificates"
            );
            return Ok((cert_path, key_path));
        }

        // Generate new certificates
        info!(
            cn = %self.cn,
            validity_days = self.validity_days,
            "Generating self-signed certificates for gRPC TLS"
        );

        self.generate(&cert_path, &key_path)?;

        Ok((cert_path, key_path))
    }

    /// Generate new self-signed certificate and key.
    fn generate(&self, cert_path: &Path, key_path: &Path) -> io::Result<()> {
        // Generate key pair
        let key_pair = KeyPair::generate().map_err(|e| {
            io::Error::other(format!("Failed to generate key pair: {}", e))
        })?;

        // Create certificate parameters
        let mut params = CertificateParams::new(vec![self.cn.clone()]).map_err(|e| {
            io::Error::other(
                format!("Failed to create certificate params: {}", e),
            )
        })?;

        // Subject
        params
            .distinguished_name
            .push(DnType::CommonName, &self.cn);
        params
            .distinguished_name
            .push(DnType::OrganizationName, "tokio_php");
        params
            .distinguished_name
            .push(DnType::OrganizationalUnitName, "gRPC Auto-Generated");

        // Validity period
        let now = OffsetDateTime::now_utc();
        params.not_before = now;
        params.not_after = now + Duration::days(self.validity_days as i64);

        // Subject Alternative Names
        params.subject_alt_names = vec![
            SanType::DnsName(self.cn.clone().try_into().unwrap()),
            SanType::DnsName("localhost".to_string().try_into().unwrap()),
            SanType::IpAddress(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
            SanType::IpAddress(IpAddr::V6(Ipv6Addr::LOCALHOST)),
        ];

        // Key usage
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];

        // Save expiry date before consuming params
        let expires = params.not_after.date();

        // Generate self-signed certificate
        let cert = params.self_signed(&key_pair).map_err(|e| {
            io::Error::other(
                format!("Failed to generate certificate: {}", e),
            )
        })?;

        // Write certificate
        fs::write(cert_path, cert.pem())?;

        // Write private key
        fs::write(key_path, key_pair.serialize_pem())?;

        // Set restrictive permissions on key file (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(key_path, fs::Permissions::from_mode(0o600))?;
        }

        info!(
            cert = %cert_path.display(),
            key = %key_path.display(),
            cn = %self.cn,
            expires = %expires,
            "Generated self-signed gRPC certificate"
        );

        Ok(())
    }

    /// Check if existing certificates are valid.
    fn certs_valid(&self, cert_path: &Path, key_path: &Path) -> bool {
        // Both files must exist
        if !cert_path.exists() || !key_path.exists() {
            return false;
        }

        // Try to parse and check expiry
        match self.check_cert_expiry(cert_path) {
            Ok(valid) => valid,
            Err(e) => {
                warn!(error = %e, "Failed to validate existing certificate, will regenerate");
                false
            }
        }
    }

    /// Check if certificate is not expired and has >30 days validity.
    fn check_cert_expiry(&self, cert_path: &Path) -> io::Result<bool> {
        let cert_pem = fs::read(cert_path)?;

        // Parse PEM
        let (_, pem) = x509_parser::pem::parse_x509_pem(&cert_pem).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Invalid PEM: {}", e))
        })?;

        // Parse X.509
        let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid X.509: {}", e),
            )
        })?;

        // Check validity
        let validity = cert.validity();
        let now = OffsetDateTime::now_utc();
        let now_asn1 = x509_parser::time::ASN1Time::from_timestamp(now.unix_timestamp())
            .map_err(|e| io::Error::other(format!("Time error: {}", e)))?;

        // Must not be expired
        if now_asn1 > validity.not_after {
            info!("Certificate expired, will regenerate");
            return Ok(false);
        }

        // Must have more than 30 days remaining
        let expires_timestamp = validity.not_after.timestamp();
        let remaining_days = (expires_timestamp - now.unix_timestamp()) / 86400;

        if remaining_days < 30 {
            info!(
                remaining_days = remaining_days,
                "Certificate expiring soon, will regenerate"
            );
            return Ok(false);
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_generate_certs() {
        let temp_dir = TempDir::new().unwrap();
        let generator = AutoCertGenerator::new("test.local", 365, temp_dir.path());

        let (cert_path, key_path) = generator.ensure_certs().unwrap();

        assert!(cert_path.exists());
        assert!(key_path.exists());

        // Verify cert content
        let cert_pem = fs::read_to_string(&cert_path).unwrap();
        assert!(cert_pem.contains("BEGIN CERTIFICATE"));

        // Verify key content
        let key_pem = fs::read_to_string(&key_path).unwrap();
        assert!(key_pem.contains("BEGIN PRIVATE KEY"));
    }

    #[test]
    fn test_reuse_existing_certs() {
        let temp_dir = TempDir::new().unwrap();
        let generator = AutoCertGenerator::new("test.local", 365, temp_dir.path());

        // Generate first time
        let (cert_path1, _) = generator.ensure_certs().unwrap();
        let cert1 = fs::read_to_string(&cert_path1).unwrap();

        // Call again - should reuse
        let (cert_path2, _) = generator.ensure_certs().unwrap();
        let cert2 = fs::read_to_string(&cert_path2).unwrap();

        assert_eq!(cert1, cert2);
    }
}
