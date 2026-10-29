//! Literal and regex matching with original UTF-8 byte ranges for text highlighting.

use std::ops::Range;

use regex::Regex;

#[derive(Clone, Debug)]
pub(super) enum Matcher {
    Contains(String),
    Exact(String),
    Regex(Regex),
}

impl Matcher {
    pub(super) fn matches(&self, text: &str) -> bool {
        match self {
            Self::Contains(value) => text.to_lowercase().contains(value),
            Self::Exact(value) => text.to_lowercase() == *value,
            Self::Regex(regex) => regex.is_match(text),
        }
    }

    pub(super) fn ranges(&self, text: &str) -> Vec<Range<usize>> {
        let value = match self {
            Self::Regex(regex) => {
                return regex
                    .find_iter(text)
                    .filter(|hit| !hit.is_empty())
                    .map(|hit| hit.range())
                    .collect();
            }
            Self::Exact(value) => {
                return if !text.is_empty() && text.to_lowercase() == *value {
                    std::iter::once(0..text.len()).collect()
                } else {
                    Vec::new()
                };
            }
            Self::Contains(value) if value.is_empty() => return Vec::new(),
            Self::Contains(value) => value,
        };
        let folded = text.to_lowercase();
        let mut offset = 0;
        let boundaries: Vec<_> = text
            .char_indices()
            .map(|(start, ch)| {
                let length = ch.to_lowercase().map(char::len_utf8).sum::<usize>();
                let folded = offset..offset + length;
                offset += length;
                (folded, start..start + ch.len_utf8())
            })
            .collect();
        folded
            .match_indices(value)
            .map(|(start, value)| {
                let first = boundaries.partition_point(|(folded, _)| folded.end <= start);
                let last =
                    boundaries.partition_point(|(folded, _)| folded.start < start + value.len());
                boundaries[first].1.start..boundaries[last - 1].1.end
            })
            .collect()
    }
}
