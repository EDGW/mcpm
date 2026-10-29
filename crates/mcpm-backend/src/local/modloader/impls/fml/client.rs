//! Client version conventions shared by Forge and NeoForge.

/// Same shape as HMCL's `^([0-9.]+)-(?<forge>[0-9.]+)(-([0-9.]+))?$`.
/// Nonmatching versions retain their entire original string.
pub(super) fn forge_version(version: &str) -> &str {
    let mut parts = version.split('-');
    let minecraft = parts.next().unwrap_or_default();
    let Some(loader) = parts.next() else {
        return version;
    };
    let suffix = parts.next();
    let numeric =
        |part: &str| !part.is_empty() && part.bytes().all(|ch| ch.is_ascii_digit() || ch == b'.');
    if numeric(minecraft) && numeric(loader) && suffix.is_none_or(numeric) && parts.next().is_none()
    {
        loader
    } else {
        version
    }
}

pub(super) const NEOFORGE_LIBRARIES: &[&str] = &[
    "net.neoforged.fancymodloader:core",
    "net.neoforged.fancymodloader:loader",
];
