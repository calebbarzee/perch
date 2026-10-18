//! Fixtures for Perch cross-crate and adversarial tests.
//!
//! The fixtures build records from raw JSON with an independent canonical
//! encoder (RFC 8785 restricted to integers, contract 4.2) and sign them with
//! ed25519-dalek directly. Tests therefore check `perch-core` and the other
//! crates against a second implementation of the contract, not against
//! themselves.

pub mod fixture;
pub mod sut;
pub mod vectors;
pub mod world;
