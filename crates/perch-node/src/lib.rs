//! # perch-node
//!
//! The native Perch node (ARCHITECTURE 8.1, 8.3): WebSocket sync, the HTTP
//! API, JSONL persistence, and static serving of the web app.
//!
//! | Module | Purpose |
//! |---|---|
//! | [`config`] | Settings and limits |
//! | [`node`] | Shared state: store, policy, sessions, address book, redaction |
//! | [`http`] | Public HTTP API |
//! | [`admin`] | Operator controls on a loopback address |
//! | [`ws`] | Drives `perch_sync::Session` over WebSocket connections |
//! | [`peers`] | Outbound connections with backoff |
//! | [`admission`] | Admission tiers |

pub mod admin;
pub mod admission;
pub mod config;
pub mod http;
pub mod node;
pub mod peers;
pub mod tls;
pub mod ws;
