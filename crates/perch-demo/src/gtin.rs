//! GTIN construction for demo products.
//!
//! Demo GTINs use GS1 prefixes 20–29 (restricted circulation, for internal
//! use within a company), so they do not collide with real trade items
//! ([GS1 General Specifications, section 2.1.11](https://www.gs1.org/standards/barcodes-epcrfid-id-keys/gs1-general-specifications)).

/// GS1 check digit for `digits` (the GTIN without its check digit).
pub fn check_digit(digits: &str) -> u8 {
    let sum: u32 = digits
        .bytes()
        .rev()
        .enumerate()
        .map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 3 } else { 1 })
        .sum();
    ((10 - sum % 10) % 10) as u8
}

/// A GTIN of `len` digits (8, 12, 13, or 14) starting with `prefix` and
/// ending with a valid check digit. `n` fills the middle, zero-padded.
///
/// # Panics
/// If the prefix and `n` do not fit in `len - 1` digits.
pub fn make(len: usize, prefix: &str, n: u64) -> String {
    let middle = len - 1 - prefix.len();
    let body = format!("{prefix}{n:0middle$}");
    assert_eq!(body.len(), len - 1, "GTIN body does not fit");
    let c = check_digit(&body);
    format!("{body}{c}")
}

/// `gtin` with a wrong check digit (for invalid-record scenarios).
pub fn corrupt(gtin: &str) -> String {
    let (body, last) = gtin.split_at(gtin.len() - 1);
    let d = (last.as_bytes()[0] - b'0' + 1) % 10;
    format!("{body}{d}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use perch_core::product_key::normalize_gtin;

    #[test]
    fn known_check_digits() {
        // EAN-13 example from GS1: 400638133393 → 1.
        assert_eq!(check_digit("400638133393"), 1);
        // UPC-A example: 03600029145 → 2.
        assert_eq!(check_digit("03600029145"), 2);
    }

    #[test]
    fn made_gtins_validate_in_core() {
        for (len, p) in [(8, "20"), (12, "2"), (13, "20"), (14, "120")] {
            for n in [0, 1, 42, 99_999] {
                let g = make(len, p, n);
                assert_eq!(g.len(), len);
                assert!(normalize_gtin(&g).is_ok(), "{g}");
                assert!(normalize_gtin(&corrupt(&g)).is_err(), "{g}");
            }
        }
    }
}
