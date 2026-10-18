//! Reconnect delays (docs/sync-protocol.md 8.2): 1 s, doubling to at most
//! 60 s, with ±20 % jitter.

pub const BASE_MS: f64 = 1000.0;
pub const MAX_MS: f64 = 60_000.0;

/// Delay before reconnect attempt `attempt` (0-based). `unit` is a random
/// number in [0, 1) used for jitter.
pub fn delay_ms(attempt: u32, unit: f64) -> f64 {
    let base = (BASE_MS * 2f64.powi(attempt.min(16) as i32)).min(MAX_MS);
    let jitter = 0.8 + 0.4 * unit.clamp(0.0, 1.0);
    base * jitter
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_and_caps() {
        assert_eq!(delay_ms(0, 0.5), 1000.0);
        assert_eq!(delay_ms(1, 0.5), 2000.0);
        assert_eq!(delay_ms(5, 0.5), 32_000.0);
        assert_eq!(delay_ms(6, 0.5), 60_000.0);
        assert_eq!(delay_ms(40, 0.5), 60_000.0);
    }

    #[test]
    fn jitter_is_twenty_percent() {
        assert_eq!(delay_ms(0, 0.0), 800.0);
        assert!((delay_ms(0, 1.0) - 1200.0).abs() < 1e-9);
    }
}
