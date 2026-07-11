//! Convert a user-supplied faucet amount into base units.

use anyhow::{Result, anyhow, bail};

/// Resolve a faucet `amount` to base units, scaled by a token's `decimals`.
///
/// Normally `amount` is a decimal number of whole units — `"1.5"` with 6
/// decimals → `1_500_000`. With `raw`, it's taken as an exact base-unit integer
/// (no scaling), for wei/fri-level precision.
pub fn to_base_units(amount: &str, decimals: u32, raw: bool) -> Result<u128> {
    if raw {
        return amount
            .parse::<u128>()
            .map_err(|_| anyhow!("raw amount '{amount}' must be a whole number of base units"));
    }
    let (int_part, frac_part) = amount.split_once('.').unwrap_or((amount, ""));
    if int_part.is_empty() && frac_part.is_empty() {
        bail!("'{amount}' is not a valid amount");
    }
    let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if !all_digits(int_part) || !all_digits(frac_part) {
        bail!("'{amount}' is not a valid decimal amount");
    }
    let decimals = decimals as usize;
    if frac_part.len() > decimals {
        bail!("'{amount}' has more than {decimals} decimal place(s) for this token");
    }
    // Concatenate the integer and fractional digits, right-padding the fraction
    // to `decimals` places, giving an exact integer number of base units.
    let mut digits = String::with_capacity(int_part.len() + decimals);
    digits.push_str(if int_part.is_empty() { "0" } else { int_part });
    digits.push_str(frac_part);
    for _ in 0..(decimals - frac_part.len()) {
        digits.push('0');
    }
    digits
        .parse::<u128>()
        .map_err(|_| anyhow!("amount '{amount}' is too large"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_whole_and_fractional_amounts() {
        assert_eq!(to_base_units("100", 6, false).unwrap(), 100_000_000);
        assert_eq!(to_base_units("1.5", 6, false).unwrap(), 1_500_000);
        assert_eq!(to_base_units("0.000001", 6, false).unwrap(), 1);
        assert_eq!(to_base_units(".5", 6, false).unwrap(), 500_000);
        assert_eq!(
            to_base_units("1", 18, false).unwrap(),
            1_000_000_000_000_000_000
        );
        assert_eq!(to_base_units("0", 8, false).unwrap(), 0);
    }

    #[test]
    fn raw_amounts_are_taken_as_base_units() {
        assert_eq!(to_base_units("1000000", 6, true).unwrap(), 1_000_000);
        // A fraction makes no sense in base units.
        assert!(to_base_units("1.5", 6, true).is_err());
    }

    #[test]
    fn rejects_junk_and_over_precise_amounts() {
        assert!(to_base_units("abc", 6, false).is_err());
        assert!(to_base_units("1.2.3", 6, false).is_err());
        // More fractional digits than the token has decimals.
        assert!(to_base_units("1.5000000", 6, false).is_err());
    }
}
