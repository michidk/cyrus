//! Supported numeric literals from preservation's developer-console contract.
use anyhow::{Context, Result, ensure};

/// Parse a decimal, hexadecimal, percent-prefixed or packed four-byte value.
/// Returns raw 32-bit bits, including for negative decimal values. Percent input
/// yields zero; packed values use little-endian byte order.
/// Conversion stops at the first non-digit and wraps at 32 bits. Short quoted
/// input is unsupported. Callers normalize case and supply live symbol lookup.
pub fn number(value: &str, symbol: impl FnOnce(&str) -> Result<u32>) -> Result<u32> {
    ensure!(!value.is_empty(), "missing console value");
    ensure!(value.is_ascii(), "console value requires ASCII bytes");
    if value.starts_with('%') {
        return Ok(0);
    }
    if let Some(quoted) = value.strip_prefix('"') {
        let bytes = quoted
            .as_bytes()
            .get(..4)
            .context("quoted console value requires four following bytes")?;
        return Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
    }
    if let Some(hex) = value.strip_prefix("0x") {
        return Ok(digits(hex, 16));
    }
    if let Some(decimal) = value.strip_prefix('-') {
        return Ok(digits(decimal, 10).wrapping_neg());
    }
    if value.as_bytes()[0].is_ascii_digit() {
        return Ok(digits(value, 10));
    }
    symbol(value)
}

fn digits(value: &str, radix: u32) -> u32 {
    value
        .chars()
        .map_while(|byte| byte.to_digit(radix))
        .fold(0u32, |value, digit| {
            value.wrapping_mul(radix).wrapping_add(digit)
        })
}
