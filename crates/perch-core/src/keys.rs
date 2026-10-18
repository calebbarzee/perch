//! Ed25519 keys, SHA-256, and strict verification (ARCHITECTURE 4.2, 4.3).
//!
//! Signing is [RFC 8032](https://www.rfc-editor.org/rfc/rfc8032) Ed25519
//! (pure, no context). Verification is strict: it rejects non-canonical `A`
//! or `R`, small-order `A` or `R`, and `S ≥ L`, and checks `[S]B = R + [k]A`
//! without cofactor multiplication. This is `verify_strict` in
//! [ed25519-dalek](https://docs.rs/ed25519-dalek), plus an explicit check
//! that `A` and `R` are canonical encodings (dalek accepts some
//! non-canonical encodings of a y coordinate ≥ p).

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::ids::{AuthorId, Hex32, Sig};

/// SHA-256 of `data`.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().into()
}

/// SHA-256 of `data` as a [`Hex32`].
pub fn sha256_hex32(data: &[u8]) -> Hex32 {
    Hex32(sha256(data))
}

/// Why a signature did not verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureError {
    /// The public key is not a canonical encoding of a curve point.
    #[error("public key is not a canonical curve point")]
    BadPublicKey,
    /// The public key has small order.
    #[error("public key has small order")]
    WeakPublicKey,
    /// `R` is not a canonical encoding, or `S ≥ L`.
    #[error("signature is not canonically encoded")]
    NonCanonicalSignature,
    /// The verification equation does not hold (or `R` has small order).
    #[error("signature does not verify")]
    Invalid,
}

/// An Ed25519 key pair derived from a 32-byte seed (the RFC 8032 secret key).
///
/// Keys never need randomness from this crate: callers pass a seed. On native
/// targets, [`Keypair::generate`] reads the operating system's random source.
#[derive(Clone)]
pub struct Keypair {
    signing: SigningKey,
    author: AuthorId,
}

impl core::fmt::Debug for Keypair {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Keypair").field("author", &self.author).finish_non_exhaustive()
    }
}

impl Keypair {
    /// Derives the key pair from a 32-byte seed.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        let signing = SigningKey::from_bytes(&seed);
        let author = AuthorId(signing.verifying_key().to_bytes());
        Self { signing, author }
    }

    /// Parses a seed written as 64 lowercase hex characters.
    pub fn from_seed_hex(hex: &str) -> Result<Self, crate::ids::HexError> {
        crate::ids::decode_hex::<32>(hex).map(Self::from_seed)
    }

    /// Generates a key pair from the operating system's random source.
    /// Native targets only; on wasm, callers pass a seed to [`Keypair::from_seed`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).expect("operating system random source failed");
        Self::from_seed(seed)
    }

    /// The 32-byte seed. Treat it as secret.
    pub fn seed(&self) -> [u8; 32] {
        self.signing.to_bytes()
    }

    /// The seed as 64 lowercase hex characters. Treat it as secret.
    pub fn seed_hex(&self) -> String {
        crate::ids::encode_hex(&self.seed())
    }

    /// The public key, which is the author ID.
    pub fn author(&self) -> AuthorId {
        self.author
    }

    /// Signs `domain || message`. Use a domain prefix from ARCHITECTURE 4.3.
    pub fn sign_with_domain(&self, domain: &[u8], message: &[u8]) -> Sig {
        let mut buf = Vec::with_capacity(domain.len() + message.len());
        buf.extend_from_slice(domain);
        buf.extend_from_slice(message);
        self.sign_raw(&buf)
    }

    /// Signs `message` as given (no domain prefix).
    pub fn sign_raw(&self, message: &[u8]) -> Sig {
        Sig(self.signing.sign(message).to_bytes())
    }
}

/// Field prime p = 2^255 − 19, little-endian.
const P_LE: [u8; 32] = {
    let mut p = [0xffu8; 32];
    p[0] = 0xed;
    p[31] = 0x7f;
    p
};

/// True if the low 255 bits of a compressed point (the y coordinate) are < p.
fn y_is_canonical(enc: &[u8; 32]) -> bool {
    let mut y = *enc;
    y[31] &= 0x7f;
    for i in (0..32).rev() {
        if y[i] != P_LE[i] {
            return y[i] < P_LE[i];
        }
    }
    false // y == p
}

/// Strictly verifies `sig` over `domain || message` under `author`.
pub fn verify_with_domain(
    author: &AuthorId,
    domain: &[u8],
    message: &[u8],
    sig: &Sig,
) -> Result<(), SignatureError> {
    let mut buf = Vec::with_capacity(domain.len() + message.len());
    buf.extend_from_slice(domain);
    buf.extend_from_slice(message);
    verify_strict(author, &buf, sig)
}

/// Strictly verifies `sig` over `message` under `author`. See the module
/// documentation for the rules.
pub fn verify_strict(author: &AuthorId, message: &[u8], sig: &Sig) -> Result<(), SignatureError> {
    let a = author.as_bytes();
    if !y_is_canonical(a) {
        return Err(SignatureError::BadPublicKey);
    }
    let mut r = [0u8; 32];
    r.copy_from_slice(&sig.0[..32]);
    if !y_is_canonical(&r) {
        return Err(SignatureError::NonCanonicalSignature);
    }
    let vk = VerifyingKey::from_bytes(a).map_err(|_| SignatureError::BadPublicKey)?;
    if vk.is_weak() {
        return Err(SignatureError::WeakPublicKey);
    }
    // from_bytes checks only the top 3 bits of S; verify_strict checks S < L.
    let signature = Signature::from_bytes(&sig.0);
    vk.verify_strict(message, &signature).map_err(|_| {
        // Distinguish S >= L for clearer errors.
        let mut s = [0u8; 32];
        s.copy_from_slice(&sig.0[32..]);
        if !scalar_is_canonical(&s) {
            SignatureError::NonCanonicalSignature
        } else {
            SignatureError::Invalid
        }
    })
}

/// Group order L = 2^252 + 27742317777372353535851937790883648493, little-endian.
const L_LE: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0x10,
];

fn scalar_is_canonical(s: &[u8; 32]) -> bool {
    for i in (0..32).rev() {
        if s[i] != L_LE[i] {
            return s[i] < L_LE[i];
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc8032_test_1() {
        // RFC 8032 section 7.1, TEST 1 (empty message).
        let seed = crate::ids::decode_hex::<32>("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60").unwrap();
        let kp = Keypair::from_seed(seed);
        assert_eq!(kp.author().to_hex(), "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        let sig = kp.sign_raw(b"");
        assert_eq!(
            sig.to_hex(),
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
        );
        assert_eq!(verify_strict(&kp.author(), b"", &sig), Ok(()));
        assert_eq!(verify_strict(&kp.author(), b"x", &sig), Err(SignatureError::Invalid));
    }

    #[test]
    fn sha256_known_answer() {
        assert_eq!(
            crate::ids::encode_hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn domain_separation() {
        let kp = Keypair::from_seed([7; 32]);
        let s = kp.sign_with_domain(crate::limits::RECORD_DOMAIN, b"m");
        assert!(verify_with_domain(&kp.author(), crate::limits::RECORD_DOMAIN, b"m", &s).is_ok());
        assert!(verify_with_domain(&kp.author(), crate::limits::RECEIPT_DOMAIN, b"m", &s).is_err());
    }

    #[test]
    fn rejects_small_order_key() {
        // The identity point (y = 1) has small order.
        let mut ident = [0u8; 32];
        ident[0] = 1;
        let sig = Sig([0; 64]);
        assert_eq!(verify_strict(&AuthorId(ident), b"", &sig), Err(SignatureError::WeakPublicKey));
    }

    #[test]
    fn rejects_small_order_r() {
        let kp = Keypair::from_seed([5; 32]);
        let good = kp.sign_raw(b"m");
        // Encodings of small-order points: identity, order 2 (y = -1), order 4 (y = 0),
        // and an order-8 point.
        let small = [
            "0100000000000000000000000000000000000000000000000000000000000000",
            "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a",
        ];
        for hex in small {
            let r = crate::ids::decode_hex::<32>(hex).unwrap();
            let mut sig = good;
            sig.0[..32].copy_from_slice(&r);
            assert!(verify_strict(&kp.author(), b"m", &sig).is_err(), "{hex}");
            // The same encodings as public keys are weak.
            assert!(verify_strict(&AuthorId(r), b"m", &good).is_err(), "{hex}");
        }
    }

    #[test]
    fn rejects_noncanonical_key_and_r() {
        // y = p + 1 encodes the same point as y = 1, non-canonically.
        let mut enc = P_LE;
        enc[0] = 0xee;
        assert!(!y_is_canonical(&enc));
        assert_eq!(verify_strict(&AuthorId(enc), b"", &Sig([0; 64])), Err(SignatureError::BadPublicKey));
        let kp = Keypair::from_seed([1; 32]);
        let mut sig = kp.sign_raw(b"m");
        sig.0[..32].copy_from_slice(&enc);
        assert_eq!(verify_strict(&kp.author(), b"m", &sig), Err(SignatureError::NonCanonicalSignature));
    }

    #[test]
    fn rejects_s_not_reduced() {
        let kp = Keypair::from_seed([2; 32]);
        let mut sig = kp.sign_raw(b"m");
        // S + L is a valid equation solution but not canonical.
        let mut s = [0u8; 32];
        s.copy_from_slice(&sig.0[32..]);
        let mut carry = 0u16;
        for i in 0..32 {
            let t = u16::from(s[i]) + u16::from(L_LE[i]) + carry;
            s[i] = t as u8;
            carry = t >> 8;
        }
        if s[31] & 0xe0 == 0 {
            sig.0[32..].copy_from_slice(&s);
            assert_eq!(verify_strict(&kp.author(), b"m", &sig), Err(SignatureError::NonCanonicalSignature));
        }
    }

    #[test]
    fn seed_roundtrip() {
        let kp = Keypair::from_seed([9; 32]);
        assert_eq!(kp.seed(), [9; 32]);
        assert_eq!(Keypair::from_seed_hex(&kp.seed_hex()).unwrap().author(), kp.author());
        assert!(format!("{kp:?}").contains("author"));
        assert!(!format!("{kp:?}").contains("0909"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn generate_differs() {
        assert_ne!(Keypair::generate().author(), Keypair::generate().author());
    }
}
