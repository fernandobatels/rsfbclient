//! Firebird fixed-point values backed by a scaled integer.
use std::{convert::TryFrom, fmt};

/// An exact fixed-point value. Its value is the coefficient times ten raised
/// to the signed scale. For example, coefficient 1234 and scale -2 mean 12.34.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedDecimal {
    coefficient: i128,
    scale: i8,
}

impl FixedDecimal {
    /// Construct a value from its Firebird coefficient and signed scale.
    pub const fn new(coefficient: i128, scale: i8) -> Self {
        Self { coefficient, scale }
    }

    /// Return the unscaled integer.
    pub const fn coefficient(self) -> i128 {
        self.coefficient
    }

    /// Return the base-ten exponent, negative for fractional digits.
    pub const fn scale(self) -> i8 {
        self.scale
    }

    /// Return the coefficient if this value can bind as a scaled BIGINT.
    pub fn as_i64_coefficient(self) -> Option<i64> {
        if self.scale < -18 {
            None
        } else {
            i64::try_from(self.coefficient).ok()
        }
    }
}

impl fmt::Display for FixedDecimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.scale >= 0 {
            self.coefficient.fmt(f)?;
            for _ in 0..self.scale {
                if self.coefficient != 0 {
                    f.write_str("0")?;
                }
            }
            return Ok(());
        }

        let digits = self.coefficient.unsigned_abs().to_string();
        let fraction = usize::from(self.scale.unsigned_abs());
        if self.coefficient < 0 {
            f.write_str("-")?;
        }
        if digits.len() <= fraction {
            f.write_str("0.")?;
            for _ in 0..fraction - digits.len() {
                f.write_str("0")?;
            }
            f.write_str(&digits)
        } else {
            let (whole, fractional) = digits.split_at(digits.len() - fraction);
            write!(f, "{whole}.{fractional}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_keeps_fraction_width_and_handles_the_signed_minimum() {
        assert_eq!(FixedDecimal::new(-42, -5).to_string(), "-0.00042");
        assert_eq!(FixedDecimal::new(0, -4).to_string(), "0.0000");
        assert_eq!(FixedDecimal::new(120, -2).to_string(), "1.20");
        assert_eq!(FixedDecimal::new(-12, 2).to_string(), "-1200");
        assert_eq!(
            FixedDecimal::new(i128::MIN, -1).to_string(),
            "-17014118346046923173168730371588410572.8"
        );
    }
}
