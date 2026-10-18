//! Measures strict Ed25519 verification cost (docs/threat-model.md section 9).
//! Run with: cargo test -p perch-tests --release --test verify_cost -- --ignored --nocapture

use ed25519_dalek::{Signer, SigningKey};

#[test]
#[ignore]
fn measure_verify_strict() {
    let k = SigningKey::from_bytes(&[7u8; 32]);
    let msg = vec![0u8; 400];
    let sig = k.sign(&msg);
    let vk = k.verifying_key();
    let n = 20_000;
    let t = std::time::Instant::now();
    for _ in 0..n {
        assert!(vk.verify_strict(&msg, &sig).is_ok());
    }
    let per = t.elapsed().as_secs_f64() / n as f64 * 1e6;
    println!("verify_strict: {per:.1} µs per signature over {n} runs");
}
