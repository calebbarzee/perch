//! Outbound TLS: a node dials a `wss://` peer through a TLS proxy with a
//! self-signed certificate, trusted only via `--ca-file`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use perch_core::{Keypair, Unsigned, sign_record};
use serde_json::json;

/// Starts a TLS-terminating proxy on 127.0.0.1 forwarding to `target`.
/// Returns the proxy port and the certificate PEM.
fn tls_proxy(target: String) -> (u16, String) {
    let ck = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let pem = ck.cert.pem();
    let cert = ck.cert.der().clone();
    let key = rustls_pki_types::PrivateKeyDer::Pkcs8(ck.signing_key.serialize_der().into());
    let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(cfg));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        rt.block_on(async move {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(l.local_addr().unwrap().port()).unwrap();
            loop {
                let Ok((sock, _)) = l.accept().await else { continue };
                let acceptor = acceptor.clone();
                let target = target.clone();
                tokio::spawn(async move {
                    let Ok(mut tls) = acceptor.accept(sock).await else { return };
                    let Ok(mut up) = tokio::net::TcpStream::connect(&target).await else { return };
                    let _ = tokio::io::copy_bidirectional(&mut tls, &mut up).await;
                });
            }
        });
    });
    (rx.recv().unwrap(), pem)
}

#[test]
fn k_dials_wss_peer_with_ca_file() {
    let dir = tmpdir("tls");
    let a = spawn(&dir.join("a"), &[]);
    let (port, pem) = tls_proxy(a.addr.clone());
    let ca = dir.join("ca.pem");
    std::fs::write(&ca, pem).unwrap();
    let url = format!("wss://localhost:{port}/v1/sync");
    let b = spawn(&dir.join("b"), &["--peer".into(), url.clone(), "--ca-file".into(), ca.display().to_string()]);
    let c = spawn(&dir.join("c"), &["--peer".into(), url.clone()]);
    let k = Keypair::from_seed([88; 32]);
    let body = json!({ "name": "Over TLS", "role": "buyer" }).as_object().unwrap().clone();
    let r = sign_record(&k, Unsigned { seq: 1, prev: None, ts: 1, kind: "profile".into(), replaces: None, body }).unwrap();
    assert_eq!(a.post("/v1/records", &r.to_json_string()).status, 200);
    let path = format!("/v1/records/{}", r.id());
    assert!(
        wait_until(Duration::from_secs(30), || b.get(&path).status == 200),
        "B did not sync over wss://:\n{}",
        b.log_text()
    );
    let st = b.status();
    assert_eq!(st["peers"][0]["url"], url.as_str());
    assert_eq!(st["peers"][0]["state"], "live");
    // Without the CA file the self-signed certificate is refused.
    assert!(wait_until(Duration::from_secs(10), || c.log_text().contains("connect failed")));
    assert_eq!(c.get(&path).status, 404);
    assert!(c.log_text().to_lowercase().contains("certificate"), "{}", c.log_text());
}
