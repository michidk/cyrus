//! Supported numeric literals from preservation's developer-console contract.
use anyhow::{Context, Result, bail, ensure};

/// Parse a decimal, hexadecimal, percent-prefixed or packed four-byte value.
/// Returns raw 32-bit bits, including for negative decimal values. Percent input
/// yields zero; packed values use little-endian byte order.
/// Short quoted input and symbolic lookup are unsupported. Callers normalize case.
pub fn number(value: &str) -> Result<u32> {
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
        return u32::from_str_radix(hex, 16).context("invalid hexadecimal console value");
    }
    if value.starts_with('-') {
        return value
            .parse::<i32>()
            .map(|v| v as u32)
            .context("invalid signed console value");
    }
    if value.bytes().all(|byte| byte.is_ascii_digit()) {
        return value.parse().context("console value exceeds 32 bits");
    }
    bail!("unsupported console value {value:?}; symbolic lookup is unavailable")
}
