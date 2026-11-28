//! Java Properties codec: selective/full decoding, lossless key upsert, and no-clobber RCON creation.

use std::{
    borrow::Cow,
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Cursor, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};

use super::config::LaunchConfig;

/// Parses a strict RCON port value (`1..=65535`, no surrounding whitespace).
pub fn parse_rcon_port(raw: &str) -> Result<u16> {
    let port = raw
        .parse::<u16>()
        .with_context(|| format!("invalid rcon.port {raw:?}"))?;
    ensure!(port > 0, "rcon.port must be between 1 and 65535");
    Ok(port)
}

/// Minecraft's conventional RCON port.
pub const DEFAULT_RCON_PORT: u16 = 25575;

/// Selected raw values are decoded lazily, so disabled features do not validate unused settings.
pub struct Properties {
    values: HashMap<String, Value>,
}

enum Value {
    Raw { bytes: Vec<u8>, line: usize },
    Decoded(String),
}

impl Properties {
    pub fn read_selected(path: &Path, keys: &[&str]) -> Result<Option<Self>> {
        Self::read_file(path, Some(keys))
    }

    /// Parses every key and value, allowing callers to distinguish a malformed document
    /// from invalid typed settings. Selected reads retain their existing lazy semantics.
    pub fn read(path: &Path) -> Result<Option<Self>> {
        Self::read_file(path, None)
    }

    fn read_file(path: &Path, keys: Option<&[&str]>) -> Result<Option<Self>> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("failed to read properties {path:?}"));
            }
        };
        let mut reader = BufReader::new(file);
        read_reader(&mut reader, keys).map(Some)
    }

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        self.values
            .get(key)
            .map(|value| match value {
                Value::Decoded(value) => Ok(value.clone()),
                Value::Raw { bytes, line } => decode(bytes)
                    .with_context(|| format!("invalid properties value for {key} at line {line}")),
            })
            .transpose()
    }
}

/// Decodes all Java Properties entries from an ISO-8859-1 byte slice (last declaration wins).
pub fn parse_bytes(bytes: &[u8]) -> Result<HashMap<String, String>> {
    let mut reader = Cursor::new(bytes);
    let properties = read_reader(&mut reader, None)?;
    Ok(properties
        .values
        .into_iter()
        .map(|(key, value)| match value {
            Value::Decoded(decoded) => (key, decoded),
            Value::Raw { .. } => unreachable!("full parse eagerly decodes every value"),
        })
        .collect())
}

/// Inserts or updates `key` in a Java Properties document while preserving comments,
/// blank lines, unrelated logical lines, and original line endings byte-for-byte.
///
/// If `key` appears multiple times, earlier declarations are dropped and the last
/// occurrence is replaced so Java's last-wins semantics remain unambiguous.
pub fn upsert_key(existing: Option<&[u8]>, key: &str, value: &str) -> Result<Vec<u8>> {
    let Some(bytes) = existing else {
        let mut output = Vec::new();
        encode_entry(&mut output, key, value, b"\n");
        return Ok(output);
    };

    let default_eol = detect_eol(bytes);
    let mut matches: Vec<(usize, usize, &[u8])> = Vec::new();
    let mut logical = Vec::new();
    let mut continuing = false;
    let mut span_start = 0;
    let mut cursor = 0;

    while cursor < bytes.len() {
        let line_start = cursor;
        while cursor < bytes.len() && !matches!(bytes[cursor], b'\r' | b'\n') {
            cursor += 1;
        }
        let content_end = cursor;
        if bytes.get(cursor) == Some(&b'\r') {
            cursor += 1;
            if bytes.get(cursor) == Some(&b'\n') {
                cursor += 1;
            }
        } else if bytes.get(cursor) == Some(&b'\n') {
            cursor += 1;
        }
        let line_end = cursor;
        let line_eol = &bytes[content_end..line_end];

        let content = trim_leading_space(&bytes[line_start..content_end]);
        if !continuing {
            if content.is_empty() || matches!(content[0], b'#' | b'!') {
                continue;
            }
            logical.clear();
            span_start = line_start;
        }
        logical.extend_from_slice(content);
        continuing = ends_with_continuation(content);
        if continuing {
            logical.pop();
            continue;
        }
        if logical_key_matches(&logical, key) {
            matches.push((span_start, line_end, line_eol));
        }
    }
    if continuing && logical_key_matches(&logical, key) {
        matches.push((span_start, bytes.len(), b""));
    }

    let mut output = Vec::with_capacity(bytes.len() + key.len() + value.len() + 4);
    if let Some((&(last_start, last_end, last_eol), earlier)) = matches.split_last() {
        let mut emitted = 0;
        for &(start, end, _) in earlier {
            output.extend_from_slice(&bytes[emitted..start]);
            emitted = end;
        }
        output.extend_from_slice(&bytes[emitted..last_start]);
        let eol = if !last_eol.is_empty() {
            last_eol
        } else {
            default_eol
        };
        encode_entry(&mut output, key, value, eol);
        output.extend_from_slice(&bytes[last_end..]);
    } else {
        output.extend_from_slice(bytes);
        if !bytes.is_empty() && !matches!(bytes.last(), Some(b'\n' | b'\r')) {
            output.extend_from_slice(default_eol);
        }
        if continuing {
            if bytes.ends_with(b"\r") && default_eol == b"\n" {
                output.push(b'\r');
            } else {
                output.extend_from_slice(default_eol);
            }
        }
        encode_entry(&mut output, key, value, default_eol);
    }

    Ok(output)
}

/// Creates only the three RCON properties without replacing an existing configuration.
/// Passwords are encoded with Java Properties escaping, including Unicode and line breaks.
pub fn create_rcon(path: &Path, port: u16, password: &str) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("failed to create RCON properties {path:?}"))?;
    let mut content = Vec::new();
    let _ = writeln!(content, "enable-rcon=true\nrcon.port={port}");
    encode_entry(&mut content, "rcon.password", password, b"\n");
    file.write_all(&content)
        .with_context(|| format!("failed to write RCON properties {path:?}"))
}

/// Returns `true` when a Minecraft version string is known to be at or after `1.9-pre4`
/// (supports classic `1.x[.y][-preN|-rcN]`, modern year-based `26.x`, and weekly snapshots `YYwWWa`).
pub fn supports_rcon_properties(version: &str) -> bool {
    let version = version.trim();
    if version.is_empty() {
        return false;
    }
    if let Some((year, week)) = parse_weekly_snapshot(version) {
        return year > 16 || (year == 16 && week >= 14);
    }
    let prefix_len = version
        .find(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .unwrap_or(version.len());
    let numeric = &version[..prefix_len];
    if numeric.is_empty() {
        return false;
    }
    let mut parts = Vec::new();
    for segment in numeric.split('.') {
        if segment.is_empty() {
            return false;
        }
        let Ok(value) = segment.parse::<u32>() else {
            return false;
        };
        parts.push(value);
    }
    let major = parts[0];
    if major >= 2 {
        return true;
    }
    if major != 1 || parts.len() < 2 {
        return false;
    }
    let minor = parts[1];
    if minor >= 10 {
        return true;
    }
    if minor < 9 {
        return false;
    }
    if parts.len() >= 3 {
        return true;
    }
    let suffix = version[prefix_len..].trim();
    if suffix.is_empty() {
        return true;
    }
    let lower = suffix.to_ascii_lowercase();
    if let Some(rest) = lower
        .strip_prefix("-rc")
        .or_else(|| lower.strip_prefix(" rc"))
    {
        let rest = rest.trim_start_matches(['-', ' ']);
        return !rest.is_empty()
            && rest.bytes().all(|b| b.is_ascii_digit())
            && rest.parse::<u32>().is_ok_and(|n| n >= 1);
    }
    if let Some(rest) = lower
        .strip_prefix("-pre-release-")
        .or_else(|| lower.strip_prefix(" pre-release "))
        .or_else(|| lower.strip_prefix("-pre"))
    {
        let rest = rest.trim_start_matches(['-', ' ']);
        return !rest.is_empty()
            && rest.bytes().all(|b| b.is_ascii_digit())
            && rest.parse::<u32>().is_ok_and(|n| n >= 4);
    }
    false
}

fn parse_weekly_snapshot(version: &str) -> Option<(u32, u32)> {
    let lower = version.to_ascii_lowercase();
    let (year_str, rest) = lower.split_once('w')?;
    if year_str.len() != 2 || !year_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let week_len = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if week_len == 0 {
        return None;
    }
    let tail = &rest[week_len..];
    if tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    Some((year_str.parse().ok()?, rest[..week_len].parse().ok()?))
}

/// Resolves and validates the `server.properties` path for a server root, optional launch
/// configuration, and optional trailing application arguments (e.g. from `conf.toml`).
///
/// Rejects paths that escape the server root (via absolute paths, `..`, or symlinked ancestors),
/// target MCPM configuration files, or refer to symlinks/non-regular files.
pub fn properties_path_from_launch(
    root: &Path,
    launch: Option<&LaunchConfig>,
    extra_app_args: &[String],
) -> Result<PathBuf> {
    let canonical_root = fs::canonicalize(root)
        .with_context(|| format!("failed to canonicalize server root {root:?}"))?;

    let mut rel = PathBuf::new();
    if let Some(cwd) = launch.and_then(|l| l.process.working_directory.as_deref())
        && !cwd.is_empty()
    {
        append_safe_relative(&mut rel, Path::new(cwd), "process.workingDirectory")?;
    }

    let launch_args = launch
        .map(|l| l.application.extra_arguments.as_slice())
        .unwrap_or(&[]);
    let config_path = Path::new(properties_argument(
        launch_args.iter().chain(extra_app_args),
    )?);
    ensure!(
        config_path.file_name().is_some(),
        "--config requires a file path"
    );
    append_safe_relative(&mut rel, config_path, "--config")?;

    ensure!(
        !is_mcpm_reserved(&rel),
        "refusing to treat MCPM reserved path {:?} as server.properties",
        rel
    );

    let target = canonical_root.join(&rel);
    ensure_confined_regular_target(&canonical_root, &target)?;
    Ok(target)
}

/// Selects the last properties option from application arguments, as Minecraft does.
pub(super) fn properties_argument<'a>(
    arguments: impl IntoIterator<Item = &'a String>,
) -> Result<&'a str> {
    let mut path = "server.properties";
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if matches!(argument.as_str(), "--config" | "-c") {
            path = arguments
                .next()
                .context("properties option (--config/-c) has no path")?;
        } else if let Some(value) = argument.strip_prefix("--config=") {
            path = value;
        }
    }
    Ok(path)
}

fn append_safe_relative(base: &mut PathBuf, candidate: &Path, label: &str) -> Result<()> {
    ensure!(
        !candidate.is_absolute(),
        "{label} path {candidate:?} must stay within the server directory"
    );
    for component in candidate.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => base.push(part),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("{label} path {candidate:?} escapes the server directory");
            }
        }
    }
    Ok(())
}

fn is_mcpm_reserved(rel: &Path) -> bool {
    let mut parts = rel.components();
    let Some(Component::Normal(first)) = parts.next() else {
        return false;
    };
    if first.eq_ignore_ascii_case(".mcpm") {
        return true;
    }
    parts.next().is_none()
        && ["mcpm.json", "conf.toml", "launch.toml"]
            .iter()
            .any(|name| first.eq_ignore_ascii_case(name))
}

fn ensure_confined_regular_target(canonical_root: &Path, target: &Path) -> Result<()> {
    // Verify the deepest existing ancestor stays inside `canonical_root` (blocks symlink escape).
    let mut cursor = target.parent();
    while let Some(dir) = cursor {
        match fs::symlink_metadata(dir) {
            Ok(_) => {
                let canonical_dir = fs::canonicalize(dir)
                    .with_context(|| format!("failed to canonicalize directory {dir:?}"))?;
                let relative_dir = canonical_dir.strip_prefix(canonical_root).with_context(
                    || {
                        format!(
                            "properties path {target:?} escapes the server directory via symlink"
                        )
                    },
                )?;
                let resolved = relative_dir.join(target.strip_prefix(dir)?);
                ensure!(
                    !is_mcpm_reserved(&resolved),
                    "refusing to treat MCPM reserved path {resolved:?} as server.properties"
                );
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                cursor = dir.parent();
            }
            Err(error) => {
                return Err(error).with_context(|| format!("failed to inspect directory {dir:?}"));
            }
        }
    }
    // Check target itself if it already exists: refuse symlinks, directories, FIFOs, sockets, etc.
    match fs::symlink_metadata(target) {
        Ok(meta) => {
            ensure!(
                !meta.file_type().is_symlink(),
                "refusing to use symlink {target:?} as server.properties"
            );
            ensure!(
                meta.file_type().is_file(),
                "refusing to use non-regular file {target:?} as server.properties"
            );
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("failed to inspect {target:?}"));
        }
    }
    Ok(())
}

/// Read-only inspection of `server.properties` RCON state for `init` Console prompting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitRconProbe {
    /// `enable-rcon=true`, valid `rcon.port` (`1..=65535`), and non-empty `rcon.password`.
    AlreadyComplete,
    /// RCON is disabled, absent, or enabled with missing/invalid port or password.
    NeedsPrompt {
        currently_enabled: bool,
        existing_port: Option<u16>,
        has_password: bool,
        invalid_port: Option<String>,
    },
    /// `server.properties` exists but failed to read or parse.
    ParseFailed { reason: String },
}

/// Inspects `server.properties` RCON readiness without exposing any existing plaintext password.
pub fn inspect_rcon_for_init(path: &Path) -> InitRconProbe {
    let properties = match Properties::read(path) {
        Ok(Some(properties)) => properties,
        Ok(None) => {
            return InitRconProbe::NeedsPrompt {
                currently_enabled: false,
                existing_port: None,
                has_password: false,
                invalid_port: None,
            };
        }
        Err(error) => {
            return InitRconProbe::ParseFailed {
                reason: format!("{error:#}"),
            };
        }
    };
    let currently_enabled = match properties.get("enable-rcon") {
        Ok(Some(value)) => value.eq_ignore_ascii_case("true"),
        Ok(None) => false,
        Err(error) => {
            return InitRconProbe::ParseFailed {
                reason: format!("{error:#}"),
            };
        }
    };
    let (existing_port, invalid_port) = match properties.get("rcon.port") {
        Ok(Some(raw)) => {
            if raw.is_empty() {
                (None, None)
            } else if let Ok(port) = parse_rcon_port(&raw) {
                (Some(port), None)
            } else {
                (None, Some(raw))
            }
        }
        Ok(None) => (None, None),
        Err(error) => {
            return InitRconProbe::ParseFailed {
                reason: format!("{error:#}"),
            };
        }
    };
    let has_password = match properties.get("rcon.password") {
        Ok(Some(value)) => !value.is_empty(),
        Ok(None) => false,
        Err(error) => {
            return InitRconProbe::ParseFailed {
                reason: format!("{error:#}"),
            };
        }
    };
    if currently_enabled && existing_port.is_some() && has_password {
        InitRconProbe::AlreadyComplete
    } else {
        InitRconProbe::NeedsPrompt {
            currently_enabled,
            existing_port,
            has_password,
            invalid_port,
        }
    }
}

/// Validated write plan for explicit `--rcon` / `--rcon-port` / `--rcon-password` requests.
/// Captures the exact serialized output from the validated snapshot to avoid read/write TOCTOU.
/// Deliberately omits `Debug` to avoid leaking plaintext passwords.
pub enum PreparedRconWrite {
    Create { port: u16, password: String },
    Update { final_port: u16, bytes: Vec<u8> },
}

/// Validates and merges explicit CLI RCON options against a single snapshot of `server.properties`.
///
/// Fails if `server.properties` is unreadable/malformed or if either `rcon.port` or
/// `rcon.password` remains unspecified after applying explicit overrides.
pub fn prepare_explicit_rcon(
    path: &Path,
    port_override: Option<u16>,
    password_override: Option<String>,
) -> Result<PreparedRconWrite> {
    if let Some(port) = port_override {
        ensure!(port > 0, "RCON port must be between 1 and 65535");
    }
    if let Some(password) = &password_override {
        ensure!(!password.is_empty(), "RCON password must not be empty");
    }
    let raw = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read RCON properties from {path:?}"));
        }
    };
    let Some(raw) = raw else {
        let Some(port) = port_override else {
            bail!("server.properties does not exist at {path:?} and --rcon-port was not provided");
        };
        let Some(password) = password_override else {
            bail!(
                "server.properties does not exist at {path:?} and --rcon-password was not provided"
            );
        };
        return Ok(PreparedRconWrite::Create { port, password });
    };

    let properties = parse_bytes(&raw)
        .with_context(|| format!("failed to parse RCON properties from {path:?}"))?;

    let (final_port, write_port) = match port_override {
        Some(port) => (port, Some(port)),
        None => {
            let raw_port = properties
                .get("rcon.port")
                .filter(|value| !value.is_empty())
                .with_context(|| {
                    format!(
                        "rcon.port is not set in {path:?}; pass --rcon-port or configure it in server.properties"
                    )
                })?;
            let port = parse_rcon_port(raw_port).with_context(|| {
                format!(
                    "rcon.port in {path:?} is invalid ({raw_port:?}); pass --rcon-port or fix server.properties"
                )
            })?;
            (port, None)
        }
    };

    let write_password = match password_override {
        Some(password) => Some(password),
        None => {
            let present = properties
                .get("rcon.password")
                .is_some_and(|value| !value.is_empty());
            ensure!(
                present,
                "rcon.password is not set in {path:?}; pass --rcon-password or configure it in server.properties"
            );
            None
        }
    };

    let mut updated = upsert_key(Some(&raw), "enable-rcon", "true")?;
    if let Some(port) = write_port {
        updated = upsert_key(Some(&updated), "rcon.port", &port.to_string())?;
    }
    if let Some(password) = write_password {
        updated = upsert_key(Some(&updated), "rcon.password", &password)?;
    }

    Ok(PreparedRconWrite::Update {
        final_port,
        bytes: updated,
    })
}

/// Applies a validated [`PreparedRconWrite`] losslessly, returning the effective RCON port.
pub fn apply_prepared_rcon(path: &Path, plan: PreparedRconWrite) -> Result<u16> {
    match plan {
        PreparedRconWrite::Create { port, password } => {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create directory {parent:?}"))?;
            }
            create_rcon(path, port, &password)?;
            Ok(port)
        }
        PreparedRconWrite::Update { final_port, bytes } => {
            atomic_write_rcon(path, &bytes, true)?;
            Ok(final_port)
        }
    }
}

/// Outcome of enabling RCON from the Console interactive prompt.
pub struct InteractiveRconEnabled {
    pub port: u16,
    pub generated_password: Option<String>,
}

/// Enables RCON from an interactive Console confirmation, creating `server.properties` if absent
/// or losslessly upserting only the needed keys from a single parsed snapshot if present.
pub fn apply_interactive_enable(
    path: &Path,
    port: Option<u16>,
    generate_password: bool,
) -> Result<InteractiveRconEnabled> {
    if let Some(port) = port {
        ensure!(port > 0, "RCON port must be between 1 and 65535");
    }
    let raw = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read existing properties {path:?}"));
        }
    };
    let Some(raw) = raw else {
        let port = port.unwrap_or(DEFAULT_RCON_PORT);
        let password = uuid::Uuid::new_v4().to_string();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {parent:?}"))?;
        }
        create_rcon(path, port, &password)?;
        return Ok(InteractiveRconEnabled {
            port,
            generated_password: Some(password),
        });
    };

    let properties =
        parse_bytes(&raw).with_context(|| format!("failed to parse properties {path:?}"))?;

    let (effective_port, write_port) = match port {
        Some(port) => (port, Some(port)),
        None => match properties
            .get("rcon.port")
            .and_then(|raw| parse_rcon_port(raw).ok())
        {
            Some(existing_port) => (existing_port, None),
            None => (DEFAULT_RCON_PORT, Some(DEFAULT_RCON_PORT)),
        },
    };
    let has_password = properties
        .get("rcon.password")
        .is_some_and(|value| !value.is_empty());
    let generated_password =
        (generate_password || !has_password).then(|| uuid::Uuid::new_v4().to_string());

    let mut updated = upsert_key(Some(&raw), "enable-rcon", "true")?;
    if let Some(port) = write_port {
        updated = upsert_key(Some(&updated), "rcon.port", &port.to_string())?;
    }
    if let Some(password) = &generated_password {
        updated = upsert_key(Some(&updated), "rcon.password", password)?;
    }
    atomic_write_rcon(path, &updated, true)?;
    Ok(InteractiveRconEnabled {
        port: effective_port,
        generated_password,
    })
}

/// Losslessly sets `enable-rcon=false` on an existing readable `server.properties`.
pub fn apply_disable_rcon(path: &Path) -> Result<()> {
    let raw = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read existing properties {path:?}"));
        }
    };
    let _ = parse_bytes(&raw).with_context(|| format!("failed to parse properties {path:?}"))?;
    let updated = upsert_key(Some(&raw), "enable-rcon", "false")?;
    atomic_write_rcon(path, &updated, false)
}

fn atomic_write_rcon(path: &Path, bytes: &[u8], restrict_to_owner: bool) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let existing_permissions = if restrict_to_owner {
        None
    } else {
        fs::metadata(path).ok().map(|meta| meta.permissions())
    };
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temporary properties file in {parent:?}"))?;
    #[cfg(unix)]
    if restrict_to_owner {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to restrict permissions on {path:?}"))?;
    }
    if let Some(permissions) = existing_permissions {
        let _ = fs::set_permissions(temp.path(), permissions);
    }
    temp.write_all(bytes)
        .with_context(|| format!("failed to write properties {path:?}"))?;
    temp.flush()
        .with_context(|| format!("failed to flush properties {path:?}"))?;
    temp.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to persist properties {path:?}"))?;
    Ok(())
}

fn read_reader(reader: &mut impl BufRead, keys: Option<&[&str]>) -> Result<Properties> {
    let mut physical = Vec::new();
    let mut logical = Vec::new();
    let mut values = HashMap::new();
    let mut continuing = false;
    let mut line = 0;
    let mut start = 0;
    while physical_line(reader, &mut physical)? {
        line += 1;
        let content = trim_leading_space(&physical);
        if !continuing {
            if content.is_empty() || matches!(content[0], b'#' | b'!') {
                continue;
            }
            logical.clear();
            start = line;
        }
        logical.extend_from_slice(content);
        continuing = ends_with_continuation(content);
        if continuing {
            logical.pop();
            continue;
        }
        select(&logical, start, keys, &mut values)?;
    }
    if continuing {
        select(&logical, start, keys, &mut values)?;
    }
    Ok(Properties { values })
}

fn encode_entry(out: &mut Vec<u8>, key: &str, value: &str, eol: &[u8]) {
    encode_key(out, key);
    out.push(b'=');
    encode_value(out, value);
    out.extend_from_slice(eol);
}

fn encode_key(out: &mut Vec<u8>, key: &str) {
    encode_component(out, key, true);
}

fn encode_value(out: &mut Vec<u8>, value: &str) {
    encode_component(out, value, false);
}

fn encode_component(out: &mut Vec<u8>, text: &str, is_key: bool) {
    let mut leading_space = true;
    for ch in text.chars() {
        if ch != ' ' {
            leading_space = false;
        }
        match ch {
            '\\' => out.extend_from_slice(b"\\\\"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\u{c}' => out.extend_from_slice(b"\\f"),
            ' ' if is_key || leading_space => out.extend_from_slice(b"\\ "),
            '=' | ':' | '#' | '!' if is_key => {
                out.push(b'\\');
                out.push(ch as u8);
            }
            ch if ch.is_ascii() && !ch.is_control() => out.push(ch as u8),
            ch => {
                for unit in ch.encode_utf16(&mut [0; 2]) {
                    let _ = write!(out, "\\u{unit:04X}");
                }
            }
        }
    }
}

fn space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | 0x0c)
}

fn trim_leading_space(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !space(*byte))
        .unwrap_or(bytes.len());
    &bytes[start..]
}

fn ends_with_continuation(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

fn detect_eol(bytes: &[u8]) -> &'static [u8] {
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\r' {
            return if bytes.get(cursor + 1) == Some(&b'\n') {
                b"\r\n"
            } else {
                b"\n"
            };
        }
        if bytes[cursor] == b'\n' {
            return b"\n";
        }
        cursor += 1;
    }
    b"\n"
}

fn physical_line(reader: &mut impl BufRead, line: &mut Vec<u8>) -> Result<bool> {
    line.clear();
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            return Ok(!line.is_empty());
        }
        if let Some(end) = bytes.iter().position(|byte| matches!(*byte, b'\n' | b'\r')) {
            let cr = bytes[end] == b'\r';
            line.extend_from_slice(&bytes[..end]);
            reader.consume(end + 1);
            if cr && reader.fill_buf()?.first() == Some(&b'\n') {
                reader.consume(1);
            }
            return Ok(true);
        }
        let length = bytes.len();
        line.extend_from_slice(bytes);
        reader.consume(length);
    }
}

fn key_boundary(line: &[u8]) -> usize {
    let mut escaped = false;
    for (index, byte) in line.iter().copied().enumerate() {
        if !escaped && (space(byte) || matches!(byte, b'=' | b':')) {
            return index;
        }
        escaped = !escaped && byte == b'\\';
    }
    line.len()
}

fn decode_key(raw_key: &[u8]) -> Result<Cow<'_, str>> {
    if raw_key.is_ascii() && !raw_key.contains(&b'\\') {
        Ok(Cow::Borrowed(
            std::str::from_utf8(raw_key).expect("ASCII key"),
        ))
    } else {
        decode(raw_key).map(Cow::Owned)
    }
}

fn logical_key_matches(logical: &[u8], target: &str) -> bool {
    let raw_key = &logical[..key_boundary(logical)];
    decode_key(raw_key).is_ok_and(|decoded| decoded.as_ref() == target)
}

fn select(
    line: &[u8],
    number: usize,
    keys: Option<&[&str]>,
    values: &mut HashMap<String, Value>,
) -> Result<()> {
    let boundary = key_boundary(line);
    let raw_key = &line[..boundary];
    let key = match decode_key(raw_key) {
        Ok(key) => key,
        Err(_) if keys.is_some() => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("invalid Java property at line {number}"));
        }
    };
    if keys.is_some_and(|keys| !keys.contains(&key.as_ref())) {
        return Ok(());
    }
    let mut value = boundary;
    while line.get(value).is_some_and(|byte| space(*byte)) {
        value += 1;
    }
    if line
        .get(value)
        .is_some_and(|byte| matches!(*byte, b'=' | b':'))
    {
        value += 1;
    }
    while line.get(value).is_some_and(|byte| space(*byte)) {
        value += 1;
    }
    let value = if keys.is_none() {
        Value::Decoded(
            decode(&line[value..])
                .with_context(|| format!("invalid Java property at line {number}"))?,
        )
    } else {
        Value::Raw {
            bytes: line[value..].to_vec(),
            line: number,
        }
    };
    values.insert(key.into_owned(), value);
    Ok(())
}

fn unit(bytes: &[u8], position: &mut usize) -> Result<u16> {
    let digits = bytes
        .get(*position..*position + 4)
        .context("incomplete Unicode escape")?;
    let mut value = 0;
    for digit in digits {
        let digit = (*digit as char)
            .to_digit(16)
            .context("non-hexadecimal Unicode escape")?;
        value = (value << 4) | digit as u16;
    }
    *position += 4;
    Ok(value)
}

fn decode(bytes: &[u8]) -> Result<String> {
    let mut output = String::with_capacity(bytes.len());
    let mut position = 0;
    while let Some(&byte) = bytes.get(position) {
        position += 1;
        if byte != b'\\' {
            output.push(byte as char);
            continue;
        }
        let Some(&escape) = bytes.get(position) else {
            break;
        };
        position += 1;
        match escape {
            b't' => output.push('\t'),
            b'n' => output.push('\n'),
            b'r' => output.push('\r'),
            b'f' => output.push('\u{c}'),
            b'u' => {
                let high = unit(bytes, &mut position)?;
                let code = if (0xd800..=0xdbff).contains(&high) {
                    if bytes.get(position..position + 2) != Some(b"\\u") {
                        bail!("unpaired Unicode surrogate");
                    }
                    position += 2;
                    let low = unit(bytes, &mut position)?;
                    anyhow::ensure!(
                        (0xdc00..=0xdfff).contains(&low),
                        "unpaired Unicode surrogate"
                    );
                    0x10000 + (((high as u32 - 0xd800) << 10) | (low as u32 - 0xdc00))
                } else {
                    high as u32
                };
                output.push(char::from_u32(code).context("unpaired Unicode surrogate")?);
            }
            other => output.push(other as char),
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &[u8]) -> Result<Properties> {
        let mut file = tempfile::NamedTempFile::new()?;
        file.write_all(text)?;
        Ok(
            Properties::read_selected(file.path(), &["enable-rcon", "rcon.port", "rcon.password"])?
                .unwrap(),
        )
    }

    #[test]
    fn unrelated_invalid_values_and_comments_do_not_validate() -> Result<()> {
        let properties = read(b"# invalid \\uZZZZ\\\r\nenable-rcon=true\r\nmotd=\\uBAD\rmax-players=garbage\nrcon.port=25575\nrcon.password=secret\n")?;
        assert_eq!(properties.get("enable-rcon")?.as_deref(), Some("true"));
        assert_eq!(properties.get("rcon.port")?.as_deref(), Some("25575"));
        assert_eq!(properties.get("rcon.password")?.as_deref(), Some("secret"));
        Ok(())
    }

    #[test]
    fn logical_lines_escaped_keys_duplicates_and_surrogates() -> Result<()> {
        let properties = read(b"rcon\\u002eport:1\nrcon.port  = 25575\nrcon.password=one\\\r\n  two\\\\three\\uD83D\\uDE00\nenable-rcon=true\n")?;
        assert_eq!(properties.get("rcon.port")?.as_deref(), Some("25575"));
        assert_eq!(
            properties.get("rcon.password")?.as_deref(),
            Some("onetwo\\three😀")
        );
        Ok(())
    }

    #[test]
    fn unused_selected_values_are_not_decoded() -> Result<()> {
        let properties = read(b"enable-rcon=false\nrcon.port=\\uZZZZ\nrcon.password=\\uD800\n")?;
        assert_eq!(properties.get("enable-rcon")?.as_deref(), Some("false"));
        assert!(properties.get("rcon.port").is_err());
        assert!(properties.get("rcon.password").is_err());
        Ok(())
    }

    #[test]
    fn rcon_creation_roundtrips_passwords_without_property_injection_or_overwrite() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("server.properties");
        let password = "  pass\\word=:#\nother=value\r\t世界😀";
        create_rcon(&path, 25590, password)?;
        let properties = Properties::read_selected(
            &path,
            &["enable-rcon", "rcon.port", "rcon.password", "other"],
        )?
        .unwrap();
        assert_eq!(properties.get("enable-rcon")?.as_deref(), Some("true"));
        assert_eq!(properties.get("rcon.port")?.as_deref(), Some("25590"));
        assert_eq!(properties.get("rcon.password")?.as_deref(), Some(password));
        assert_eq!(properties.get("other")?, None);
        let original = std::fs::read(&path)?;
        assert!(create_rcon(&path, 1, "replacement").is_err());
        assert_eq!(std::fs::read(path)?, original);
        Ok(())
    }

    #[test]
    fn parse_bytes_matches_java_properties_edge_cases() -> Result<()> {
        let values = parse_bytes(
            b"a=old\na:new\nempty\n=x\nspace\\ key : =value  \ncontinued=hello\\\n  #part\\\r\n world\nescaped\\u003dkey=\\uD83D\\uDE00\ntrailing=abc\\",
        )?;
        for (key, value) in [
            ("", "x"),
            ("a", "new"),
            ("empty", ""),
            ("space key", "=value  "),
            ("continued", "hello#partworld"),
            ("escaped=key", "😀"),
            ("trailing", "abc"),
        ] {
            assert_eq!(values.get(key).map(String::as_str), Some(value), "{key}");
        }
        assert!(parse_bytes(br"broken=\u12xz").is_err());
        Ok(())
    }

    #[test]
    fn upsert_key_preserves_document_collapses_duplicates_and_roundtrips_escapes() -> Result<()> {
        assert_eq!(
            upsert_key(None, "enable-rcon", "true")?,
            b"enable-rcon=true\n"
        );

        let crlf_doc = b"# header\r\n\r\nrcon\\u002eport : 1\r\nmotd = Keep # verbatim\r\nrcon.port=2\\\r\n  5575\r\nmax-players=20";
        let updated = upsert_key(Some(crlf_doc), "rcon.port", "25590")?;
        assert_eq!(
            updated,
            b"# header\r\n\r\nmotd = Keep # verbatim\r\nrcon.port=25590\r\nmax-players=20"
        );

        let appended = upsert_key(Some(&updated), "online-mode", "false")?;
        assert_eq!(
            appended,
            b"# header\r\n\r\nmotd = Keep # verbatim\r\nrcon.port=25590\r\nmax-players=20\r\nonline-mode=false\r\n"
        );

        let trailing_cont = upsert_key(Some(b"other=abc\\"), "added", "1")?;
        let parsed_cont = parse_bytes(&trailing_cont)?;
        assert_eq!(parsed_cont.get("other").map(String::as_str), Some("abc"));
        assert_eq!(parsed_cont.get("added").map(String::as_str), Some("1"));

        let trailing_cr_cont = upsert_key(Some(b"other=abc\\\r"), "added", "1")?;
        let parsed_cr_cont = parse_bytes(&trailing_cr_cont)?;
        assert_eq!(parsed_cr_cont.get("other").map(String::as_str), Some("abc"));
        assert_eq!(parsed_cr_cont.get("added").map(String::as_str), Some("1"));

        let key = " #!k=e:y\\ \t\n\r\u{c}😀";
        let value = "  v a\\l=:#!\n\r\t\u{c}世界😀  ";
        let encoded = upsert_key(None, key, value)?;
        let parsed = parse_bytes(&encoded)?;
        assert_eq!(parsed.get(key).map(String::as_str), Some(value));
        Ok(())
    }

    #[test]
    fn rcon_version_threshold_matches_1_9_pre4_across_release_formats() {
        for supported in [
            "1.9-pre4",
            "1.9-pre5",
            "1.9-rc1",
            "1.9",
            "1.9.4",
            "1.12.2",
            "1.21.11",
            "26.1",
            "26.3-snapshot-1",
            "16w14a",
            "24w14a",
        ] {
            assert!(supports_rcon_properties(supported), "{supported}");
        }
        for unsupported in [
            "1.2.5",
            "1.7.10",
            "1.8.9",
            "1.9-pre1",
            "1.9-pre3",
            "1.9-pre4garbage",
            "1.9-rc1extra",
            "16w07b",
            "15w51b",
            "custom",
            "",
        ] {
            assert!(!supports_rcon_properties(unsupported), "{unsupported}");
        }
    }

    #[test]
    fn properties_path_rejects_escape_reserved_files_and_symlinks() -> Result<()> {
        let root = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let ok = properties_path_from_launch(root.path(), None, &[])?;
        assert!(ok.ends_with("server.properties"));
        let mut launch = super::super::config::vanilla_server()?.launch;
        launch.process.working_directory = Some(".".into());
        assert_eq!(
            properties_path_from_launch(root.path(), Some(&launch), &[])?,
            ok
        );

        for bad_arg in [
            "",
            ".",
            "../outside.properties",
            "/etc/passwd",
            "mcpm.json",
            ".mcpm/server.json",
            "conf.toml",
            "launch.toml",
        ] {
            assert!(
                properties_path_from_launch(
                    root.path(),
                    None,
                    &["--config".into(), bad_arg.into()],
                )
                .is_err(),
                "{bad_arg}"
            );
        }

        #[cfg(unix)]
        {
            let target = outside.path().join("secret.properties");
            fs::write(&target, "enable-rcon=true\n")?;
            std::os::unix::fs::symlink(&target, root.path().join("link.properties"))?;
            assert!(
                properties_path_from_launch(
                    root.path(),
                    None,
                    &["--config=link.properties".into()],
                )
                .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn trailing_whitespace_and_zero_ports_are_rejected_consistently() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("server.properties");
        for bad in ["25575 ", "0"] {
            fs::write(
                &path,
                format!("enable-rcon=true\nrcon.port={bad}\nrcon.password=pw\n"),
            )?;
            assert!(matches!(
                inspect_rcon_for_init(&path),
                InitRconProbe::NeedsPrompt {
                    currently_enabled: true,
                    existing_port: None,
                    has_password: true,
                    invalid_port: Some(_),
                }
            ));
            assert!(prepare_explicit_rcon(&path, None, None).is_err());
        }
        Ok(())
    }

    #[test]
    fn init_rcon_probe_explicit_and_interactive_flows_preserve_properties() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("server.properties");

        // Missing file -> NeedsPrompt with nothing configured.
        assert_eq!(
            inspect_rcon_for_init(&path),
            InitRconProbe::NeedsPrompt {
                currently_enabled: false,
                existing_port: None,
                has_password: false,
                invalid_port: None,
            }
        );

        // Explicit on missing file fails when port or password is missing, succeeds when both provided.
        assert!(prepare_explicit_rcon(&path, None, None).is_err());
        assert!(prepare_explicit_rcon(&path, Some(25580), None).is_err());
        let plan = prepare_explicit_rcon(&path, Some(25580), Some("s3cr3t".into()))?;
        assert_eq!(apply_prepared_rcon(&path, plan)?, 25580);
        assert_eq!(inspect_rcon_for_init(&path), InitRconProbe::AlreadyComplete);

        // Incomplete enable-rcon=true (empty password) -> NeedsPrompt(currently_enabled: true),
        // and declining sets enable-rcon=false losslessly.
        fs::write(
            &path,
            "# keep me\nenable-rcon=true\nrcon.port=25590\nrcon.password=\n",
        )?;
        assert_eq!(
            inspect_rcon_for_init(&path),
            InitRconProbe::NeedsPrompt {
                currently_enabled: true,
                existing_port: Some(25590),
                has_password: false,
                invalid_port: None,
            }
        );
        assert!(prepare_explicit_rcon(&path, None, None).is_err());
        apply_disable_rcon(&path)?;
        let after_disable = fs::read_to_string(&path)?;
        assert!(after_disable.contains("# keep me\n"));
        assert!(after_disable.contains("enable-rcon=false\n"));

        // Interactive enable reuses existing port and generates UUID password.
        let enabled = apply_interactive_enable(&path, None, true)?;
        assert_eq!(enabled.port, 25590);
        assert!(
            enabled
                .generated_password
                .as_deref()
                .is_some_and(|p| !p.is_empty())
        );
        assert_eq!(inspect_rcon_for_init(&path), InitRconProbe::AlreadyComplete);

        // Malformed file -> ParseFailed on probe and error on explicit.
        fs::write(&path, "bad=\\uZZZZ\n")?;
        assert!(matches!(
            inspect_rcon_for_init(&path),
            InitRconProbe::ParseFailed { .. }
        ));
        assert!(prepare_explicit_rcon(&path, Some(25575), Some("pw".into())).is_err());
        Ok(())
    }
}
