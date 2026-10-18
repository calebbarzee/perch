//! Size and range limits from ARCHITECTURE 4.2 and 9.

/// Largest integer allowed in Perch JSON: 2^53 − 1.
pub const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;
/// A record line longer than this many bytes is rejected before parsing.
pub const MAX_LINE_BYTES: usize = 131_072;
/// Maximum size of the canonical header in bytes.
pub const MAX_HEADER_BYTES: usize = 2_048;
/// Maximum size of the canonical body in bytes.
pub const MAX_BODY_BYTES: usize = 65_536;
/// Maximum JSON nesting depth. The record object is depth 1; each nested
/// object or array adds 1.
pub const MAX_DEPTH: usize = 16;
/// Signature domain prefix for record headers (ARCHITECTURE 4.3).
pub const RECORD_DOMAIN: &[u8] = b"perch/record/v1\n";
/// Signature domain prefix for receipts (ARCHITECTURE 4.3).
pub const RECEIPT_DOMAIN: &[u8] = b"perch/receipt/v1\n";
