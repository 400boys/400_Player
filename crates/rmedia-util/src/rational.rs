//! Rational numbers and timestamp scaling utilities (AVRational reproduction).

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// Sentinel timestamp representing undefined or unavailable presentation/decompression time.
pub const NOPTS_VALUE: i64 = i64::MIN;

/// Rounding modes for rescaling timestamps and rationals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rounding {
    /// Round toward zero.
    Zero = 0,
    /// Round away from zero (towards infinity).
    Inf = 1,
    /// Round toward -infinity (down).
    Down = 2,
    /// Round toward +infinity (up).
    Up = 3,
    /// Round to nearest, ties towards infinity.
    NearInf = 5,
    /// Pass min/max values untouched.
    PassMinMax = 8192,
}

/// Exact rational number with numerator and denominator (AVRational).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Rational {
    pub num: i32,
    pub den: i32,
}

impl Rational {
    /// Create a new Rational and reduce it to lowest terms.
    pub const fn new(num: i32, den: i32) -> Self {
        if den == 0 {
            return Self { num, den: 0 };
        }
        let (n, d) = if den < 0 { (-num, -den) } else { (num, den) };
        let g = gcd_const(n.unsigned_abs(), d.unsigned_abs()) as i32;
        if g > 1 {
            Self {
                num: n / g,
                den: d / g,
            }
        } else {
            Self { num: n, den: d }
        }
    }

    /// Create without reducing (useful when exact components must be preserved).
    pub const fn new_raw(num: i32, den: i32) -> Self {
        Self { num, den }
    }

    /// Invert numerator and denominator.
    pub const fn invert(self) -> Self {
        Self::new(self.den, self.num)
    }

    /// Convert to floating-point value.
    pub fn to_f64(self) -> f64 {
        if self.den == 0 {
            if self.num > 0 {
                f64::INFINITY
            } else if self.num < 0 {
                f64::NEG_INFINITY
            } else {
                f64::NAN
            }
        } else {
            self.num as f64 / self.den as f64
        }
    }

    /// Reduce the rational to its lowest terms.
    pub fn reduce(self) -> Self {
        Self::new(self.num, self.den)
    }

    /// Check if the rational is valid (non-zero denominator).
    pub const fn is_valid(self) -> bool {
        self.den != 0
    }
}

const fn gcd_const(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

impl Add for Rational {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        if self.den == rhs.den {
            Self::new(self.num + rhs.num, self.den)
        } else {
            let num = (self.num as i64) * (rhs.den as i64) + (rhs.num as i64) * (self.den as i64);
            let den = (self.den as i64) * (rhs.den as i64);
            let g = gcd_u64(num.unsigned_abs(), den.unsigned_abs()) as i64;
            let (n, d) = if g > 1 { (num / g, den / g) } else { (num, den) };
            Self::new(n as i32, d as i32)
        }
    }
}

impl Sub for Rational {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        self + (-rhs)
    }
}

impl Mul for Rational {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self::Output {
        let num = (self.num as i64) * (rhs.num as i64);
        let den = (self.den as i64) * (rhs.den as i64);
        let g = gcd_u64(num.unsigned_abs(), den.unsigned_abs()) as i64;
        let (n, d) = if g > 1 { (num / g, den / g) } else { (num, den) };
        Self::new(n as i32, d as i32)
    }
}

impl Div for Rational {
    type Output = Self;
    fn div(self, rhs: Self) -> Self::Output {
        self * rhs.invert()
    }
}

impl Neg for Rational {
    type Output = Self;
    fn neg(self) -> Self::Output {
        Self {
            num: -self.num,
            den: self.den,
        }
    }
}

fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Rational {
    fn cmp(&self, other: &Self) -> Ordering {
        let lhs = (self.num as i64) * (other.den as i64);
        let rhs = (other.num as i64) * (self.den as i64);
        lhs.cmp(&rhs)
    }
}

/// Rescale a timestamp using integer arithmetic and rounding mode (av_rescale_rnd).
pub fn rescale_rnd(a: i64, b: i64, c: i64, rnd: Rounding) -> i64 {
    if c <= 0 || b < 0 {
        return NOPTS_VALUE;
    }
    if a == NOPTS_VALUE {
        return NOPTS_VALUE;
    }
    if a == 0 || b == 0 {
        return 0;
    }

    let is_neg = a < 0;
    let abs_a = a.unsigned_abs();
    let b = b as u64;
    let c = c as u64;

    // Use 128-bit integer multiplication to prevent 64-bit overflow
    let prod = (abs_a as u128) * (b as u128);
    let c_128 = c as u128;

    let res = match rnd {
        Rounding::Zero => prod / c_128,
        Rounding::Inf => (prod + c_128 - 1) / c_128,
        Rounding::Down => {
            if is_neg {
                (prod + c_128 - 1) / c_128
            } else {
                prod / c_128
            }
        }
        Rounding::Up => {
            if is_neg {
                prod / c_128
            } else {
                (prod + c_128 - 1) / c_128
            }
        }
        Rounding::NearInf => (prod + c_128 / 2) / c_128,
        Rounding::PassMinMax => {
            if a == i64::MIN || a == i64::MAX {
                return a;
            }
            (prod + c_128 / 2) / c_128
        }
    };

    if is_neg {
        -(res as i64)
    } else {
        res as i64
    }
}

/// Rescale a timestamp with default rounding (av_rescale).
pub fn rescale(a: i64, b: i64, c: i64) -> i64 {
    rescale_rnd(a, b, c, Rounding::NearInf)
}

/// Rescale a timestamp from one timebase rational to another (av_rescale_q).
pub fn rescale_q(a: i64, bq: Rational, cq: Rational) -> i64 {
    rescale_q_rnd(a, bq, cq, Rounding::NearInf)
}

/// Rescale a timestamp from one timebase to another with explicit rounding (av_rescale_q_rnd).
pub fn rescale_q_rnd(a: i64, bq: Rational, cq: Rational, rnd: Rounding) -> i64 {
    let b = (bq.num as i64) * (cq.den as i64);
    let c = (cq.num as i64) * (bq.den as i64);
    rescale_rnd(a, b, c, rnd)
}

/// Compare two timestamps with different timebases (av_compare_ts).
pub fn compare_ts(ts_a: i64, tb_a: Rational, ts_b: i64, tb_b: Rational) -> Ordering {
    if ts_a == NOPTS_VALUE && ts_b == NOPTS_VALUE {
        return Ordering::Equal;
    }
    if ts_a == NOPTS_VALUE {
        return Ordering::Less;
    }
    if ts_b == NOPTS_VALUE {
        return Ordering::Greater;
    }

    let a = (ts_a as i128) * (tb_a.num as i128) * (tb_b.den as i128);
    let b = (ts_b as i128) * (tb_b.num as i128) * (tb_a.den as i128);
    a.cmp(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rational_arithmetic() {
        let r1 = Rational::new(1, 2);
        let r2 = Rational::new(1, 3);
        assert_eq!(r1 + r2, Rational::new(5, 6));
        assert_eq!(r1 - r2, Rational::new(1, 6));
        assert_eq!(r1 * r2, Rational::new(1, 6));
        assert_eq!(r1 / r2, Rational::new(3, 2));
    }

    #[test]
    fn test_rescale() {
        let pts = 90000;
        let tb_in = Rational::new(1, 90000);
        let tb_out = Rational::new(1, 1000);
        assert_eq!(rescale_q(pts, tb_in, tb_out), 1000);
    }

    #[test]
    fn test_compare_ts() {
        let tb1 = Rational::new(1, 1000);
        let tb2 = Rational::new(1, 90000);
        assert_eq!(compare_ts(1000, tb1, 90000, tb2), Ordering::Equal);
        assert_eq!(compare_ts(1001, tb1, 90000, tb2), Ordering::Greater);
    }
}
