//! Lossless Minecraft version ordering and catalog-backed abbreviations for search presentation.

use std::collections::{BTreeMap, BTreeSet};

/// Full identifiers for details and copying. Series descend; numeric patches ascend.
pub(super) fn expanded(versions: &[String]) -> Vec<String> {
    let (groups, other) = groups(versions);
    let mut result = Vec::new();
    for ((major, minor), group) in groups.into_iter().rev() {
        let prefix = format!("{major}.{minor}");
        if group.base {
            result.push(prefix.clone());
        }
        result.extend(
            group
                .patches
                .iter()
                .map(|patch| format!("{prefix}.{patch}")),
        );
        result.extend(
            group
                .suffixes
                .iter()
                .map(|suffix| format!("{prefix}.{suffix}")),
        );
    }
    result.extend(other);
    result
}

/// Wildcards require an exact nonempty match with the source's full prefix catalog.
/// Without a catalog, only explicit numeric patch ranges are abbreviated.
pub(super) fn compact(versions: &[String], catalog: Option<&[String]>) -> Vec<String> {
    let (mut groups, mut other) = groups(versions);
    let collapsed = major_wildcards(versions, catalog);
    for &major in &collapsed {
        groups.entry((major, 0)).or_default();
    }
    other.retain(|version| {
        !version
            .split_once('.')
            .and_then(|(major, _)| number(major))
            .is_some_and(|major| collapsed.contains(&major))
    });
    let mut result = Vec::new();
    let mut last_major = None;
    for ((major, minor), group) in groups.into_iter().rev() {
        if collapsed.contains(&major) {
            if last_major != Some(major) {
                result.push(format!("{major}.x"));
            }
            last_major = Some(major);
            continue;
        }
        let prefix = format!("{major}.{minor}");
        let dotted = format!("{prefix}.");
        let supported: BTreeSet<_> = group
            .patches
            .iter()
            .map(ToString::to_string)
            .chain(group.suffixes.iter().cloned())
            .collect();
        let complete = catalog.is_some_and(|catalog| {
            let known: BTreeSet<_> = catalog
                .iter()
                .filter_map(|version| version.strip_prefix(&dotted).map(str::to_owned))
                .collect();
            !known.is_empty() && known == supported
        });
        if complete {
            result.push(format!(
                "{prefix}{}",
                if group.base { "(.x)" } else { ".x" }
            ));
            continue;
        }
        if group.base {
            result.push(prefix.clone());
        }
        let mut patches = ranges(&group.patches);
        patches.extend(group.suffixes);
        if !patches.is_empty() {
            result.push(format!("{prefix}.{}", patches.join("/")));
        }
    }
    result.extend(other);
    result
}

#[derive(Default)]
struct Group {
    base: bool,
    patches: BTreeSet<u64>,
    suffixes: BTreeSet<String>,
}

type Groups = BTreeMap<(u64, u64), Group>;

/// A non-1 major collapses only when every catalog identifier under its prefix is supported.
fn major_wildcards(versions: &[String], catalog: Option<&[String]>) -> BTreeSet<u64> {
    let Some(catalog) = catalog else {
        return BTreeSet::new();
    };
    let majors: BTreeSet<_> = versions
        .iter()
        .filter_map(|version| version.split_once('.').and_then(|(major, _)| number(major)))
        .filter(|&major| major != 1)
        .collect();
    majors
        .into_iter()
        .filter(|major| {
            let prefix = format!("{major}.");
            let supported: BTreeSet<_> = versions
                .iter()
                .filter(|version| version.starts_with(&prefix))
                .cloned()
                .collect();
            let known: BTreeSet<_> = catalog
                .iter()
                .filter(|version| version.starts_with(&prefix))
                .cloned()
                .collect();
            !known.is_empty() && supported == known
        })
        .collect()
}

fn groups(versions: &[String]) -> (Groups, Vec<String>) {
    let mut groups = Groups::new();
    let mut other = BTreeSet::new();
    for version in versions {
        let mut parts = version.split('.');
        let parsed = parts
            .next()
            .and_then(number)
            .zip(parts.next().and_then(number));
        let suffix = parts.next();
        if let Some(key) = parsed
            && parts.next().is_none()
            && suffix.is_none_or(|value| !value.is_empty())
        {
            let group = groups.entry(key).or_default();
            match suffix {
                None => group.base = true,
                Some(value) => match number(value) {
                    Some(patch) => {
                        group.patches.insert(patch);
                    }
                    None => {
                        group.suffixes.insert(value.to_owned());
                    }
                },
            }
        } else {
            other.insert(version.clone());
        }
    }
    // Unrecognized names stay intact and last, with numeric-leading names ordered numerically.
    let mut other: Vec<_> = other.into_iter().collect();
    other.sort_by(|a, b| {
        let key = |value: &str| {
            let mut parts = value.split('.');
            let major = parts.next().and_then(number);
            (major, major.and_then(|_| parts.next().and_then(number)))
        };
        key(b).cmp(&key(a)).then_with(|| a.cmp(b))
    });
    (groups, other)
}

/// Noncanonical or oversized numeric strings are preserved verbatim as suffixes/other names.
fn number(value: &str) -> Option<u64> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return None;
    }
    value.parse().ok()
}

fn ranges(patches: &BTreeSet<u64>) -> Vec<String> {
    let mut result = Vec::new();
    let mut patches = patches.iter().copied().peekable();
    while let Some(start) = patches.next() {
        let mut end = start;
        while patches
            .peek()
            .is_some_and(|&next| end.checked_add(1) == Some(next))
        {
            end = patches.next().unwrap();
        }
        result.push(if start == end {
            start.to_string()
        } else {
            format!("{start}~{end}")
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }

    #[test]
    fn wildcards_require_all_catalog_suffixes_and_preserve_the_base_distinction() {
        let catalog = strings(&["1.20", "1.20.1", "1.20.2", "1.20.3", "1.21.1"]);
        let mut support = strings(&["1.20.3", "1.20.1", "1.20.2", "1.20.2"]);
        assert_eq!(compact(&support, Some(&catalog)), ["1.20.x"]);
        support.push("1.20".into());
        assert_eq!(compact(&support, Some(&catalog)), ["1.20(.x)"]);
        assert_eq!(compact(&support, None), ["1.20", "1.20.1~3"]);
        let mut catalog = catalog;
        catalog.push("1.20.6-snapshot".into());
        assert_eq!(compact(&support, Some(&catalog)), ["1.20", "1.20.1~3"]);
        assert_eq!(compact(&support, Some(&[])), ["1.20", "1.20.1~3"]);
        support.push("1.20.99".into());
        assert_eq!(compact(&support, Some(&catalog)), ["1.20", "1.20.1~3/99"]);
        assert_eq!(compact(&strings(&["1.20"]), Some(&catalog)), ["1.20"]);
    }

    #[test]
    fn non_one_series_require_the_full_catalog_regardless_of_segment_count() {
        let catalog = strings(&["22.1", "22.2", "1.20", "1.21"]);
        let support = strings(&["1.20", "22.2", "1.21", "22.1"]);
        assert_eq!(compact(&support, Some(&catalog)), ["22.x", "1.21", "1.20"]);
        assert_eq!(compact(&support, None), ["22.2", "22.1", "1.21", "1.20"]);
        assert_eq!(compact(&strings(&["22.1"]), Some(&catalog)), ["22.1"]);
        let mut catalog = catalog;
        catalog.push("22.1.1".into());
        assert_eq!(
            compact(&support, Some(&catalog)),
            ["22.2", "22.1", "1.21", "1.20"]
        );
        let mut support = support;
        support.extend(strings(&["22.1.1", "22.2.3-snapshot", "22.preview.1"]));
        catalog.extend(strings(&["22.2.3-snapshot", "22.preview.1"]));
        assert_eq!(compact(&support, Some(&catalog)), ["22.x", "1.21", "1.20"]);
        support.push("22.3.1".into());
        assert!(!compact(&support, Some(&catalog)).contains(&"22.x".into()));
    }

    #[test]
    fn numeric_series_descend_patches_compress_and_other_names_remain_lossless_at_the_end() {
        let support = strings(&[
            "1.20.10",
            "1.20.1",
            "1.20.8",
            "1.20.5",
            "1.20.7",
            "1.20.3",
            "1.20.6",
            "1.20.6-snapshot",
            "1.20.01",
            "1.20.18446744073709551616",
            "1.20.18446744073709551615",
            "26.1",
            "26.10.2",
            "1.9.4",
            "1.21.1",
            "1.20",
            "24w14a",
            "b1.7.3",
            "1.20-pre1",
            "1.20.1-pre.1",
            "26.preview",
            "1.20.1",
            "1.20.",
            "1.20.测试",
        ]);
        assert_eq!(
            compact(&support, None),
            [
                "26.10.2",
                "26.1",
                "1.21.1",
                "1.20",
                "1.20.1/3/5~8/10/18446744073709551615/01/18446744073709551616/6-snapshot/测试",
                "1.9.4",
                "26.preview",
                "1.20.",
                "1.20.1-pre.1",
                "1.20-pre1",
                "24w14a",
                "b1.7.3",
            ]
        );
        let full = expanded(&support);
        assert_eq!(&full[..4], ["26.10.2", "26.1", "1.21.1", "1.20"]);
        assert_eq!(
            full.iter().collect::<BTreeSet<_>>(),
            support.iter().collect::<BTreeSet<_>>()
        );
        assert!(
            !full
                .iter()
                .any(|value| value.contains('~') || value.contains('/'))
        );
    }
}
