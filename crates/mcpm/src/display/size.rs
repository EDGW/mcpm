//! Binary file sizes with exact byte counts rendered as secondary information.

use super::style::OutputStyle;

/// Selects a binary unit for a content byte count.
pub(crate) fn binary(bytes: u128) -> String {
    const UNITS: [&str; 8] = ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.2}{}", UNITS[unit])
}

/// Keeps the exact count, including its parentheses, muted.
pub(crate) fn size(bytes: u128, style: &OutputStyle) -> String {
    format!(
        "{} {}",
        binary(bytes),
        style.muted(format!("({bytes} bytes)"))
    )
}
