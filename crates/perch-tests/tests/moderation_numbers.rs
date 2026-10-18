//! Reproduces the probability tables in docs/moderation.md (section 9 method).

fn ln_choose(n: u64, k: u64) -> f64 {
    let lg = |x: u64| (1..=x).map(|i| (i as f64).ln()).sum::<f64>();
    lg(n) - lg(k) - lg(n - k)
}

/// P(X >= k) for X ~ Binomial(n, p).
fn binom_ge(n: u64, p: f64, k: u64) -> f64 {
    let below: f64 = (0..k)
        .map(|i| (ln_choose(n, i) + i as f64 * p.ln() + (n - i) as f64 * (1.0 - p).ln()).exp())
        .sum();
    (1.0 - below).max(0.0)
}

fn pct(x: f64) -> f64 {
    (x * 10_000.0).round() / 100.0
}

#[test]
fn false_hide_table_matches_doc() {
    // docs/moderation.md section 3.1
    assert_eq!(pct(binom_ge(1_000, 0.0005, 3)), 1.44);
    assert_eq!(pct(binom_ge(10_000, 0.0005, 3)), 87.54);
    assert_eq!(pct(binom_ge(1_000, 0.001, 3)), 8.02);
    assert_eq!(pct(binom_ge(10_000, 0.001, 3)), 99.72);
    assert_eq!(pct(binom_ge(1_000, 0.001, 5)), 0.36);
    assert_eq!(pct(binom_ge(10_000, 0.001, 5)), 97.08);
    assert_eq!(pct(binom_ge(10_000, 0.001, 10)), 54.21);
    assert_eq!(pct(binom_ge(10_000, 0.001, 20)), 0.34);
    assert_eq!(pct(binom_ge(100, 0.001, 3)), 0.02);
}

#[test]
fn jury_capture_table_matches_doc() {
    // docs/moderation.md section 4.4: P(colluders hold a majority)
    let maj = |n: u64| n / 2 + 1;
    assert!((binom_ge(5, 0.10, maj(5)) - 0.00856).abs() < 1e-5);
    assert!((binom_ge(9, 0.10, maj(9)) - 0.00089).abs() < 1e-5);
    assert!((binom_ge(9, 0.20, maj(9)) - 0.01958).abs() < 1e-5);
    assert!((binom_ge(21, 0.30, maj(21)) - 0.02639).abs() < 1e-5);
    // Grinding: expected tries = 1 / P
    assert_eq!((1.0 / binom_ge(5, 0.10, 3)).round(), 117.0);
    assert_eq!((1.0 / binom_ge(9, 0.10, 5)).round(), 1122.0);
}
