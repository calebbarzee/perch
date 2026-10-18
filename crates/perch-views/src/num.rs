//! Exact numbers (docs/views.md section 12).
//!
//! Views compute with integers and exact fractions. A fractional result is
//! rounded half to even to hundredths ([`round2`]) and emitted as a JSON
//! number by [`Num`].

use core::cmp::Ordering;

use serde::{Serialize, Serializer};

/// An exact fraction `num / den` with `den > 0`.
#[derive(Debug, Clone, Copy)]
pub struct Ratio {
    /// Numerator.
    pub num: i128,
    /// Denominator, > 0.
    pub den: i128,
}

impl Ratio {
    /// `num / den`. A denominator below 1 is treated as 1 (never used by
    /// views; it keeps the function total).
    pub fn new(num: i128, den: i128) -> Ratio {
        Ratio { num, den: den.max(1) }
    }
}

impl PartialEq for Ratio {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl Eq for Ratio {}
impl PartialOrd for Ratio {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Ratio {
    fn cmp(&self, o: &Self) -> Ordering {
        (self.num * o.den).cmp(&(o.num * self.den))
    }
}

/// Rounds `x` half to even to hundredths; returns the integer `k` with
/// result `k / 100` (ARCHITECTURE 7.7).
pub fn round2(x: Ratio) -> i64 {
    let neg = x.num < 0;
    let m = x.num.unsigned_abs() * 100;
    let q = x.den as u128;
    let mut k = m / q;
    let r = m - k * q;
    if 2 * r > q || (2 * r == q && k % 2 == 1) {
        k += 1;
    }
    let k = k as i64;
    if neg { -k } else { k }
}

/// A number with two decimal places, stored as hundredths. Serializes as a
/// JSON integer when whole, otherwise as the nearest binary64 value, which
/// prints as the exact decimal (docs/views.md section 12.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Num(pub i64);

impl Num {
    /// `round2(x)`.
    pub fn round(x: Ratio) -> Num {
        Num(round2(x))
    }

    /// A whole number.
    pub fn int(n: i64) -> Num {
        Num(n * 100)
    }

    /// The decimal text: `4`, `4.2`, `4.25`, `-0.5`.
    pub fn text(&self) -> String {
        let k = self.0;
        let sign = if k < 0 { "-" } else { "" };
        let a = k.unsigned_abs();
        let (i, f) = (a / 100, a % 100);
        if f == 0 {
            format!("{sign}{i}")
        } else if f % 10 == 0 {
            format!("{sign}{i}.{}", f / 10)
        } else {
            format!("{sign}{i}.{f:02}")
        }
    }

    /// The value as `f64`.
    pub fn as_f64(&self) -> f64 {
        self.0 as f64 / 100.0
    }
}

impl Serialize for Num {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.0 % 100 == 0 { s.serialize_i64(self.0 / 100) } else { s.serialize_f64(self.as_f64()) }
    }
}

/// `round2(sum / count)` as a [`Num`], or `None` when `count` is 0.
pub fn mean(sum: u64, count: u64) -> Option<Num> {
    (count > 0).then(|| Num::round(Ratio::new(i128::from(sum), i128::from(count))))
}

/// Lower median of sorted values (ARCHITECTURE 7.7).
pub fn lower_median(sorted: &[u64]) -> Option<u64> {
    if sorted.is_empty() { None } else { Some(sorted[(sorted.len() - 1) / 2]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_even_examples() {
        assert_eq!(round2(Ratio::new(107, 40)), 268); // 2.675
        assert_eq!(round2(Ratio::new(33, 8)), 412); // 4.125
        assert_eq!(round2(Ratio::new(35, 8)), 438); // 4.375
        assert_eq!(round2(Ratio::new(10, 3)), 333);
        assert_eq!(round2(Ratio::new(-1, 8)), -12); // -0.125
        assert_eq!(round2(Ratio::new(-3, 8)), -38); // -0.375
    }

    #[test]
    fn text_and_json() {
        for (k, t) in [(400, "4"), (420, "4.2"), (425, "4.25"), (-50, "-0.5"), (7, "0.07"), (0, "0"), (-1210, "-12.1")] {
            assert_eq!(Num(k).text(), t);
            assert_eq!(serde_json::to_string(&Num(k)).unwrap(), t);
        }
    }

    #[test]
    fn median() {
        assert_eq!(lower_median(&[1, 2, 3, 4]), Some(2));
        assert_eq!(lower_median(&[5]), Some(5));
        assert_eq!(lower_median(&[]), None);
    }
}
