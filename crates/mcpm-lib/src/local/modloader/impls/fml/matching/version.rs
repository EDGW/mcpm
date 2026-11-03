//! Maven 3 ComparableVersion ordering: numeric items, qualifiers and nested hyphen lists.
//!
//! Follows Apache Maven's ComparableVersion (3.9.x), including qualifier aliases
//! and zero normalization. Numeric items use decimal strings to avoid overflow.
//! Unsupported non-ASCII inputs and excessive nesting yield unknown comparisons.

use std::cmp::Ordering;

use super::super::super::utils::dependency::lexical;

pub(super) fn compare(left: &str, right: &str) -> Option<Ordering> {
    Some(compare_lists(&parse(left)?, &parse(right)?))
}

#[derive(Debug)]
enum Item {
    Number(String),
    Qualifier(String),
    List(Vec<Item>),
}

impl Item {
    fn number(value: &str) -> Self {
        let value = value.trim_start_matches('0');
        Self::Number(if value.is_empty() { "0" } else { value }.to_owned())
    }

    fn qualifier(value: &str, followed_by_digit: bool) -> Self {
        Self::Qualifier(
            match (value, followed_by_digit) {
                ("a", true) => "alpha",
                ("b", true) => "beta",
                ("m", true) => "milestone",
                ("ga" | "final" | "release", _) => "",
                ("cr", _) => "rc",
                _ => value,
            }
            .to_owned(),
        )
    }

    fn is_null(&self) -> bool {
        match self {
            Self::Number(value) => value == "0",
            Self::Qualifier(value) => value.is_empty(),
            Self::List(items) => items.is_empty(),
        }
    }

    fn compare_missing(&self) -> Ordering {
        match self {
            Self::Number(value) => {
                if value == "0" {
                    Ordering::Equal
                } else {
                    Ordering::Greater
                }
            }
            Self::Qualifier(value) => qualifier_key(value).cmp(&qualifier_key("")),
            Self::List(items) => items
                .iter()
                .map(Self::compare_missing)
                .find(|order| !order.is_eq())
                .unwrap_or(Ordering::Equal),
        }
    }

    fn compare(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => a.len().cmp(&b.len()).then_with(|| a.cmp(b)),
            (Self::Qualifier(a), Self::Qualifier(b)) => {
                let (rank_a, text_a) = qualifier_key(a);
                let (rank_b, text_b) = qualifier_key(b);
                rank_a.cmp(&rank_b).then_with(|| lexical(text_a, text_b))
            }
            (Self::List(a), Self::List(b)) => compare_lists(a, b),
            (Self::Number(_), _) | (Self::List(_), Self::Qualifier(_)) => Ordering::Greater,
            _ => Ordering::Less,
        }
    }
}

fn parse(value: &str) -> Option<Vec<Item>> {
    if value.is_empty() || !value.is_ascii() {
        return None;
    }
    let value = value.to_ascii_lowercase();
    let mut stack = vec![Vec::new()];
    let mut start = 0;
    let mut digit = false;
    for (index, byte) in value.bytes().enumerate() {
        match byte {
            b'.' | b'-' => {
                stack.last_mut()?.push(if start == index {
                    Item::number("0")
                } else {
                    item(&value[start..index], digit)
                });
                start = index + 1;
                if byte == b'-' {
                    push_list(&mut stack)?;
                }
            }
            b'0'..=b'9' => {
                if !digit && index > start {
                    if !stack.last()?.is_empty() {
                        push_list(&mut stack)?;
                    }
                    stack
                        .last_mut()?
                        .push(Item::qualifier(&value[start..index], true));
                    start = index;
                    push_list(&mut stack)?;
                }
                digit = true;
            }
            _ => {
                if digit && index > start {
                    stack.last_mut()?.push(Item::number(&value[start..index]));
                    start = index;
                    push_list(&mut stack)?;
                }
                digit = false;
            }
        }
    }
    if start < value.len() {
        if !digit && !stack.last()?.is_empty() {
            push_list(&mut stack)?;
        }
        stack.last_mut()?.push(item(&value[start..], digit));
    }
    while let Some(mut items) = stack.pop() {
        normalize(&mut items);
        if let Some(parent) = stack.last_mut() {
            parent.push(Item::List(items));
        } else {
            return Some(items);
        }
    }
    None
}

fn push_list(stack: &mut Vec<Vec<Item>>) -> Option<()> {
    if stack.len() >= 128 {
        return None;
    }
    stack.push(Vec::new());
    Some(())
}

fn item(value: &str, digit: bool) -> Item {
    if digit {
        Item::number(value)
    } else {
        Item::qualifier(value, false)
    }
}

fn normalize(items: &mut Vec<Item>) {
    let mut index = items.len();
    while index > 0 {
        index -= 1;
        if items[index].is_null() {
            items.remove(index);
        } else if !matches!(items[index], Item::List(_)) {
            break;
        }
    }
}

fn compare_lists(left: &[Item], right: &[Item]) -> Ordering {
    for index in 0..left.len().max(right.len()) {
        let order = match (left.get(index), right.get(index)) {
            (Some(a), Some(b)) => a.compare(b),
            (Some(a), None) => a.compare_missing(),
            (None, Some(b)) => b.compare_missing().reverse(),
            (None, None) => Ordering::Equal,
        };
        if !order.is_eq() {
            return order;
        }
    }
    Ordering::Equal
}

fn qualifier_key(value: &str) -> (u8, &str) {
    match value {
        "alpha" => (0, ""),
        "beta" => (1, ""),
        "milestone" => (2, ""),
        "rc" => (3, ""),
        "snapshot" => (4, ""),
        "" => (5, ""),
        "sp" => (6, ""),
        _ => (7, value),
    }
}
