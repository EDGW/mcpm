//! Reusable terminal widgets and themed browser building blocks.

pub(crate) mod browser;
mod highlighted_line;
pub(crate) use highlighted_line::highlighted_line;
mod matched_text;
pub(crate) use matched_text::{MatchedText, Overflow};
