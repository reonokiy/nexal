//! Self-signed TLS certificate generation for WebTransport.
//!
//! Generates a CA + server certificate pair stored at `~/.nexal/certs/`.
//! The CA cert can be distributed to clients for verification.
//! If certs already exist on disk, they are loaded without regeneration.

use std::path::{Path, PathBuf};

use rcgen::{
    CertificateParams, DistinguishedName, DnType, Issuer, IsCa, KeyPair, BasicConstraints,
    KeyUsagePurpose, SanType,
};
use tracing::info;

/// Paths to the generated certificate files.
#[derive(Debug, Clone)]
pub struct CertPaths {
    pub ca_cert: PathBuf,
    pub server_cert: PathBuf,
    pub server_key: PathBuf,
}

/// In-memory certificate material ready for use by wtransport.
#[derive(Debug, Clone)]
pub struct CertMaterial {
    pub ca_cert_pem: String,
    pub server_cert_pem: String,
    pub server_key_pem: String,
}

impl CertPaths {
    pub fn default_dir() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".nexal").join("certs"))
    }
}

/// Load existing certs from `dir`, or generate fresh ones if missing.
pub async fn ensure_certs(dir: &Path) -> Result<CertMaterial, CertError> {
    let paths = CertPaths {
        ca_cert: dir.join("ca.pem"),
        server_cert: dir.join("server.pem"),
        server_key: dir.join("server.key"),
    };

    // Try loading existing certs.
    if paths.ca_cert.exists() && paths.server_cert.exists() && paths.server_key.exists() {
        let ca = tokio::fs::read_to_string(&paths.ca_cert)
            .await
            .map_err(|e| CertError::Io(format!("read {}: {e}", paths.ca_cert.display())))?;
        let cert = tokio::fs::read_to_string(&paths.server_cert)
            .await
            .map_err(|e| CertError::Io(format!("read {}: {e}", paths.server_cert.display())))?;
        let key = tokio::fs::read_to_string(&paths.server_key)
            .await
            .map_err(|e| CertError::Io(format!("read {}: {e}", paths.server_key.display())))?;
        info!("loaded TLS certs from {}", dir.display());
        return Ok(CertMaterial {
            ca_cert_pem: ca,
            server_cert_pem: cert,
            server_key_pem: key,
        });
    }

    // Generate new certs.
    let material = generate()?;

    // Persist to disk.
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| CertError::Io(format!("mkdir {}: {e}", dir.display())))?;
    tokio::fs::write(&paths.ca_cert, &material.ca_cert_pem)
        .await
        .map_err(|e| CertError::Io(format!("write {}: {e}", paths.ca_cert.display())))?;
    tokio::fs::write(&paths.server_cert, &material.server_cert_pem)
        .await
        .map_err(|e| CertError::Io(format!("write {}: {e}", paths.server_cert.display())))?;
    tokio::fs::write(&paths.server_key, &material.server_key_pem)
        .await
        .map_err(|e| CertError::Io(format!("write {}: {e}", paths.server_key.display())))?;
    info!("generated TLS certs at {}", dir.display());
    Ok(material)
}

/// Generate a CA + server certificate in memory.
pub fn generate() -> Result<CertMaterial, CertError> {
    // ── CA ──
    let ca_key = KeyPair::generate().map_err(|e| CertError::Gen(format!("ca keygen: {e}")))?;
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages.push(KeyUsagePurpose::KeyCertSign);
    ca_params.key_usages.push(KeyUsagePurpose::CrlSign);
    let mut ca_dn = DistinguishedName::new();
    ca_dn.push(DnType::CommonName, "nexal CA");
    ca_dn.push(DnType::OrganizationName, "nexal");
    ca_params.distinguished_name = ca_dn;

    // Self-sign the CA cert (for the PEM output), then create an
    // Issuer from the same params + key for signing the server cert.
    let ca_cert_self = ca_params
        .clone()
        .self_signed(&ca_key)
        .map_err(|e| CertError::Gen(format!("ca self-sign: {e}")))?;
    let ca_cert_pem = ca_cert_self.pem();
    let ca_issuer = Issuer::new(ca_params, ca_key);

    // ── Server ──
    let server_key =
        KeyPair::generate().map_err(|e| CertError::Gen(format!("server keygen: {e}")))?;
    let mut server_params = CertificateParams::default();
    let mut server_dn = DistinguishedName::new();
    server_dn.push(DnType::CommonName, "nexal gateway");
    server_params.distinguished_name = server_dn;
    server_params
        .subject_alt_names
        .push(SanType::DnsName("localhost".try_into().map_err(|e| {
            CertError::Gen(format!("san: {e}"))
        })?));
    server_params
        .subject_alt_names
        .push(SanType::IpAddress(std::net::IpAddr::V4(
            std::net::Ipv4Addr::LOCALHOST,
        )));
    server_params
        .subject_alt_names
        .push(SanType::DnsName("host.containers.internal".try_into().map_err(|e| {
            CertError::Gen(format!("san: {e}"))
        })?));
    let server_cert = server_params
        .signed_by(&server_key, &ca_issuer)
        .map_err(|e| CertError::Gen(format!("server sign: {e}")))?;

    Ok(CertMaterial {
        ca_cert_pem,
        server_cert_pem: server_cert.pem(),
        server_key_pem: server_key.serialize_pem(),
    })
}

#[derive(Debug, Clone)]
pub enum CertError {
    Io(String),
    Gen(String),
}

impl std::fmt::Display for CertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CertError::Io(s) => write!(f, "cert io: {s}"),
            CertError::Gen(s) => write!(f, "cert gen: {s}"),
        }
    }
}

impl std::error::Error for CertError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_valid_pem() {
        let m = generate().expect("generate should succeed");
        assert!(m.ca_cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(m.server_cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(m.server_key_pem.contains("BEGIN PRIVATE KEY"));
        // CA and server certs are different.
        assert_ne!(m.ca_cert_pem, m.server_cert_pem);
    }

    #[tokio::test]
    async fn ensure_certs_generates_and_loads() {
        let dir = std::env::temp_dir().join("nexal-cert-test");
        let _ = tokio::fs::remove_dir_all(&dir).await;

        // First call generates.
        let m1 = ensure_certs(&dir).await.expect("first ensure");
        assert!(dir.join("ca.pem").exists());

        // Second call loads from disk.
        let m2 = ensure_certs(&dir).await.expect("second ensure");
        assert_eq!(m1.ca_cert_pem, m2.ca_cert_pem);
        assert_eq!(m1.server_cert_pem, m2.server_cert_pem);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
