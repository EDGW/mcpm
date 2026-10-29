//! Fabric's semantic version superset and prerelease ordering, independent of predicates.
//!
//! Numeric cores allow any number of signed-32-bit nonnegative components;
//! missing components compare as zero. Build metadata does not affect ordering.

use std::cmp::Ordering;

use super::super::super::utils::dependency::lexical;

#[derive(Debug)]
pub(super) struct SemanticVersion<'a> {
    pub components: Vec<i32>,
    pub prerelease: Option<&'a str>,
    build: Option<&'a str>,
}

impl<'a> SemanticVersion<'a> {
    pub fn parse(value: &'a str, wildcard: bool) -> Option<Self> {
        let (value, build) = value
            .split_once('+')
            .map_or((value, None), |(a, b)| (a, Some(b)));
        let (core, prerelease) = value
            .split_once('-')
            .map_or((value, None), |(a, b)| (a, Some(b)));
        if let Some(pre) = prerelease {
            if !pre.is_empty()
                && pre.split('.').any(|part| {
                    part.is_empty()
                        || !part
                            .bytes()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-')
                })
            {
                return None;
            }
        }
        let mut components = Vec::new();
        let mut has_wildcard = false;
        for part in core.split('.') {
            if wildcard && matches!(part, "x" | "X" | "*") {
                if prerelease.is_some() {
                    return None;
                }
                if !has_wildcard {
                    components.push(-1);
                }
                has_wildcard = true;
            } else {
                if has_wildcard || part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                components.push(part.parse::<i32>().ok()?);
            }
        }
        if components.first() == Some(&-1) {
            return None;
        }
        Some(Self {
            components,
            prerelease,
            build,
        })
    }

    pub fn component(&self, index: usize) -> i32 {
        self.components.get(index).copied().unwrap_or(0)
    }

    pub fn friendly(&self) -> String {
        let mut value = self
            .components
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(".");
        if let Some(pre) = self.prerelease {
            value.push('-');
            value.push_str(pre);
        }
        if let Some(build) = self.build {
            value.push('+');
            value.push_str(build);
        }
        value
    }

    pub fn compare(&self, other: &Self) -> Ordering {
        for index in 0..self.components.len().max(other.components.len()) {
            let order = self.component(index).cmp(&other.component(index));
            if !order.is_eq() {
                return order;
            }
        }
        match (self.prerelease, other.prerelease) {
            (None, None) => Ordering::Equal,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(a), Some(b)) => compare_prerelease(a, b),
        }
    }
}

fn compare_prerelease(left: &str, right: &str) -> Ordering {
    let mut a = left.split('.').filter(|s| !s.is_empty());
    let mut b = right.split('.').filter(|s| !s.is_empty());
    loop {
        let order = match (a.next(), b.next()) {
            (Some(a), Some(b)) => match (numeric(a), numeric(b)) {
                (true, true) => a.len().cmp(&b.len()).then_with(|| lexical(a, b)),
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => lexical(a, b),
            },
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (None, None) => return Ordering::Equal,
        };
        if !order.is_eq() {
            return order;
        }
    }
}

fn numeric(value: &str) -> bool {
    (value == "0" || !value.starts_with('0')) && value.bytes().all(|ch| ch.is_ascii_digit())
}
