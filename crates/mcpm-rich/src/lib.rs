//! Style-free rich text shared by every mcpm crate: semantic roles, blocks and the `rich!` macro.
//!
//! [`Rich`] describes what a piece of text means (a command, a success, a warning), never how it
//! looks. Frontends map each [`Role`] to colors and decide wrapping, labels and indentation.
//! Build values with [`rich!`], which parses its markup at compile time.

// Lets `rich!` expansions, which name `::mcpm_rich`, compile inside this crate.
extern crate self as mcpm_rich;

use std::fmt;

use serde::{Deserialize, Serialize};

pub use mcpm_macros::rich;

/// Semantic text: paragraphs and lists of role spans.
///
/// `Display` prints the plain text in `rich!` markup, so a logged value can be pasted back into
/// source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rich {
    blocks: Vec<Block>,
}

impl Rich {
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Appends the blocks of `other`, for text that depends on runtime conditions.
    pub fn extend(&mut self, other: Rich) {
        self.blocks.extend(other.blocks);
    }

    /// Entry point of the `rich!` macro; not a general constructor.
    #[doc(hidden)]
    pub fn __from_blocks(blocks: Vec<Block>) -> Self {
        Self { blocks }
    }
}

impl fmt::Display for Rich {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, block) in self.blocks.iter().enumerate() {
            if index > 0 {
                output.write_str("\n\n")?;
            }
            let (items, bullet): (Vec<&[Span]>, &str) = match block {
                Block::Paragraph(spans) => (vec![spans], ""),
                Block::List(items) => (items.iter().map(Vec::as_slice).collect(), "- "),
            };
            // `> ` blocks are the only way to write Muted without markers; prefer them when the
            // block has no Normal text, so the output reads like the source.
            let muted = !items
                .iter()
                .copied()
                .flatten()
                .any(|span| span.role == Role::Normal);
            let (quote, base) = if muted {
                ("> ", Role::Muted)
            } else {
                ("", Role::Normal)
            };
            for (line, spans) in items.into_iter().enumerate() {
                if line > 0 {
                    output.write_str("\n")?;
                }
                write!(output, "{quote}{bullet}")?;
                for span in spans {
                    match span.role {
                        role if role == base => output.write_str(&escape(&span.text))?,
                        Role::Code => write!(output, "`{}`", escape(&span.text))?,
                        role => write!(output, "[{}]({})", escape(&span.text), role.marker())?,
                    }
                }
            }
        }
        Ok(())
    }
}

/// Doubles every markup character so plain text survives a round trip through `rich!`.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '`' | '{' | '}' | '[' | ']') {
            escaped.push(ch);
        }
        escaped.push(ch);
    }
    escaped
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Block {
    /// One wrapped paragraph.
    Paragraph(Vec<Span>),
    /// Bullet items, each wrapped with a hanging indent.
    List(Vec<Vec<Span>>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub role: Role,
    pub text: String,
}

/// What a span means; frontends own the mapping to styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    /// Body text.
    Normal,
    /// Command, path, file name or configuration key.
    Code,
    /// Key value or default choice.
    Primary,
    /// Count or secondary value.
    Secondary,
    /// Emphasized text.
    Important,
    /// Background or optional detail.
    Muted,
    Success,
    Warning,
    Danger,
}

impl Role {
    /// Markup spelling inside `[text](role)`; `Normal` and `Code` have their own syntax.
    pub fn marker(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Code => "code",
            Self::Primary => "primary",
            Self::Secondary => "secondary",
            Self::Important => "important",
            Self::Muted => "muted",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Danger => "danger",
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn plain_text_is_markup_that_rebuilds_the_same_value() {
        let path = "a`b[c]{d}";
        let value = crate::rich!(
            "
            [Removed](success) `{path}` {{now}} [[x]].

            > - one [two](danger)
            > - `three`
            "
        );
        assert_eq!(
            value.to_string(),
            "[Removed](success) `a``b[[c]]{{d}}` {{now}} [[x]].\n\n> - one [two](danger)\n> - `three`"
        );
        let rebuilt = crate::rich!(
            "
            [Removed](success) `a``b[[c]]{{d}}` {{now}} [[x]].

            > - one [two](danger)
            > - `three`
            "
        );
        assert_eq!(rebuilt, value);
    }
}
