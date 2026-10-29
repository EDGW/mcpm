use std::process::Command;

use anyhow::{Context, Result};
use mcpm_backend::local::mc::client::{ClientPlatform, RuleEnvironment};
use thiserror::Error;

#[derive(Debug, Error)]
enum PlatformError {
    #[error("client commands do not support host OS {os:?}")]
    Unsupported { os: &'static str },
    #[error("system platform query failed")]
    QueryFailed,
    #[error("system query did not return an OS version")]
    MissingVersion,
}

pub(super) fn current() -> Result<RuleEnvironment> {
    let os = match std::env::consts::OS {
        "macos" => "osx",
        "linux" => "linux",
        "windows" => "windows",
        os => return Err(PlatformError::Unsupported { os }.into()),
    };
    let (arch, version) = host_details()?;
    let arch = normalize_architecture(&arch);
    let bits = architecture_bits(&arch).unwrap_or(usize::BITS as u8);
    Ok(RuleEnvironment::new(ClientPlatform {
        os: os.to_owned(),
        arch,
        version,
        bits,
    }))
}

// HMCL-compatible names shared by host defaults and explicit JVM overrides.
pub(super) fn normalize_architecture(arch: &str) -> String {
    match arch {
        "amd64" | "x64" | "x86_64" => "x86_64",
        "i386" | "i486" | "i586" | "i686" | "x86" => "x86",
        "aarch64" | "arm64" | "ARM64" => "arm64",
        "arm" | "armv7l" | "arm32" => "arm32",
        other => other,
    }
    .into()
}

pub(super) fn architecture_bits(arch: &str) -> Option<u8> {
    match arch {
        "x86" | "arm32" => Some(32),
        arch if arch.contains("64") || arch == "s390x" => Some(64),
        _ => None,
    }
}

#[cfg(unix)]
fn host_details() -> Result<(String, String)> {
    fn uname(flag: &str) -> Result<String> {
        let result = Command::new("uname")
            .arg(flag)
            .output()
            .context("failed to run uname")?;
        if !result.status.success() {
            return Err(PlatformError::QueryFailed.into());
        }
        let value = String::from_utf8(result.stdout).context("invalid uname output")?;
        let value = value.trim().to_owned();
        if value.is_empty() {
            return Err(PlatformError::MissingVersion.into());
        }
        Ok(value)
    }
    Ok((uname("-m")?, uname("-r")?))
}

#[cfg(windows)]
fn host_details() -> Result<(String, String)> {
    let result = Command::new("cmd.exe")
        .args(["/D", "/C", "ver"])
        .output()
        .context("failed to query Windows version")?;
    if !result.status.success() {
        return Err(PlatformError::QueryFailed.into());
    }
    let text = String::from_utf8_lossy(&result.stdout);
    let version = text
        .split(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .find(|value| value.contains('.') && value.bytes().any(|byte| byte.is_ascii_digit()))
        .ok_or(PlatformError::MissingVersion)?
        .to_owned();
    let arch = std::env::var("PROCESSOR_ARCHITEW6432")
        .or_else(|_| std::env::var("PROCESSOR_ARCHITECTURE"))
        .unwrap_or_else(|_| std::env::consts::ARCH.to_owned())
        .to_ascii_lowercase();
    Ok((arch, version))
}

#[cfg(not(any(unix, windows)))]
fn host_details() -> Result<(String, String)> {
    Err(PlatformError::Unsupported {
        os: std::env::consts::OS,
    }
    .into())
}
