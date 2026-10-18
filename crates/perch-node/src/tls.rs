//! Outbound TLS for `wss://` peers (rustls with the `ring` provider).
//!
//! Trust roots: the Mozilla roots of [`webpki-roots`](https://docs.rs/webpki-roots),
//! plus any certificates in `--ca-file` (PEM), for private CAs or
//! self-signed test peers. Inbound TLS is left to a reverse proxy (docs/api.md).

use std::path::Path;
use std::sync::Arc;

use rustls::RootCertStore;
use rustls_pki_types::CertificateDer;
use rustls_pki_types::pem::PemObject;
use tokio_tungstenite::Connector;

/// Builds the client configuration used for every `wss://` dial.
pub fn client_config(ca_file: Option<&Path>) -> Result<Arc<rustls::ClientConfig>, String> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(path) = ca_file {
        let mut n = 0;
        for cert in CertificateDer::pem_file_iter(path).map_err(|e| format!("--ca-file {}: {e}", path.display()))? {
            let cert = cert.map_err(|e| format!("--ca-file {}: {e}", path.display()))?;
            roots.add(cert).map_err(|e| format!("--ca-file {}: {e}", path.display()))?;
            n += 1;
        }
        if n == 0 {
            return Err(format!("--ca-file {}: no PEM certificates found", path.display()));
        }
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}

/// The connector for a URL: TLS for `wss://`, plain for `ws://`.
pub fn connector_for(url: &str, cfg: &Arc<rustls::ClientConfig>) -> Connector {
    if url.starts_with("wss://") { Connector::Rustls(cfg.clone()) } else { Connector::Plain }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wss_urls_get_a_tls_connector() {
        let cfg = client_config(None).unwrap();
        assert!(matches!(connector_for("wss://node.example.org/v1/sync", &cfg), Connector::Rustls(_)));
        assert!(matches!(connector_for("ws://127.0.0.1:7701/v1/sync", &cfg), Connector::Plain));
    }

    #[test]
    fn ca_file_is_loaded_and_checked() {
        let dir = std::env::temp_dir().join(format!("perch-tls-unit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let good = dir.join("ca.pem");
        std::fs::write(&good, cert.cert.pem()).unwrap();
        assert!(client_config(Some(&good)).is_ok());
        let empty = dir.join("empty.pem");
        std::fs::write(&empty, "not a certificate\n").unwrap();
        assert!(client_config(Some(&empty)).unwrap_err().contains("no PEM certificates"));
        assert!(client_config(Some(&dir.join("missing.pem"))).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
