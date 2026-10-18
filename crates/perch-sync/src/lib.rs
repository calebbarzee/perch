//! # perch-sync
//!
//! The Perch sync protocol ([docs/sync-protocol.md](../../../docs/sync-protocol.md))
//! as a state machine with no I/O. It compiles to `wasm32-unknown-unknown`.
//!
//! | Item | Purpose |
//! |---|---|
//! | [`Message`] | Typed protocol messages; strict parsing and JSON encoding |
//! | [`Session`] | Protocol state for one connection |
//! | [`Hub`] | All sessions of one peer, with gossip fan-out (single-threaded drivers) |
//! | [`AddressBook`] | Peer address table with per-group caps |
//! | [`sim`] | Deterministic multi-node simulation over in-memory queues |
//!
//! ## Driving a session
//!
//! ```text
//! let mut s = Session::new(id, cfg, outbound);
//! send(s.start());                                  // hello
//! on each text frame:   send(s.on_message(&mut store, text)?)   // Err: send err.to_message(), close
//! on store changes:     send(s.on_changes(&store, &store.drain_changes(), origin))
//! every 10 minutes:     send(s.refresh(&store))
//! ```
//!
//! The store must have change tracking on (`MemoryStore::set_track_changes(true)`).

pub mod address;
pub mod hub;
pub mod message;
pub mod session;
pub mod sim;

pub use address::AddressBook;
pub use hub::{Hub, Outbox, insert_batch};
pub use message::{ErrorCode, HeadEntry, Hello, Limits, Message, ParseError, PROTOCOL, Role, VERSION, WantEntry};
pub use session::{Session, SessionConfig, State, Stats, SyncError};
