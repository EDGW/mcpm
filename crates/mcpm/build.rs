//! Capture build identity, Git revision and contributors for the shared CLI frontends.

use std::{env, fs, path::Path, path::PathBuf, process::Command};

use anyhow::{Context, Result, ensure};

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    let source_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").context("missing manifest directory")?);
    if let Err(error) = watch_git(&source_dir) {
        println!(
            "cargo:warning=Git change tracking unavailable: {}",
            format!("{error:#}").replace(['\r', '\n'], " ")
        );
    }
    let revision = match revision(&source_dir) {
        Ok(revision) => Some(revision),
        Err(error) => {
            println!(
                "cargo:warning=Commit unavailable: {}",
                format!("{error:#}").replace(['\r', '\n'], " ")
            );
            None
        }
    };
    build_identity(revision.as_ref())?;
    let contributors = match contributors(&source_dir) {
        Ok(contributors) => contributors,
        Err(error) => {
            println!(
                "cargo:warning=Contributors unavailable: {}",
                format!("{error:#}").replace(['\r', '\n'], " ")
            );
            "Unavailable (Git history could not be read)\n".to_owned()
        }
    };
    let output_dir =
        PathBuf::from(env::var_os("OUT_DIR").context("missing build output directory")?);
    fs::write(output_dir.join("contributors.txt"), contributors)
        .context("failed to save build-time contributors")
}

fn build_identity(revision: Option<&Revision>) -> Result<()> {
    let profile = env::var("PROFILE").context("missing Cargo build profile")?;
    let value = |name| {
        println!("cargo:rerun-if-env-changed={name}");
        env::var(name).unwrap_or_default()
    };
    let requested = value("MCPM_OFFICIAL_RELEASE");
    let github = value("GITHUB_ACTIONS");
    let repository = value("GITHUB_REPOSITORY");
    let event = value("GITHUB_EVENT_NAME");
    let ref_type = value("GITHUB_REF_TYPE");
    let tag = value("GITHUB_REF_NAME");
    let version = env::var("CARGO_PKG_VERSION").context("missing Cargo package version")?;
    let official = profile == "release"
        && requested == "1"
        && github == "true"
        && repository == "EDGW/mcpm"
        && matches!(event.as_str(), "push" | "workflow_dispatch")
        && ref_type == "tag"
        && tag == format!("v{version}");
    let channel = if official { "official" } else { "unofficial" };
    let commit = match revision {
        Some(revision) => format!(
            "{}{}",
            revision.commit,
            if revision.modified { " + modified" } else { "" }
        ),
        None => "unavailable (Git metadata could not be read)".to_owned(),
    };
    let version = if official {
        version
    } else {
        match revision {
            Some(revision) => format!(
                "{version}.{}{}",
                &revision.commit[..6],
                if revision.modified { ".modified" } else { "" }
            ),
            None => format!("{version}.unknown"),
        }
    };
    println!("cargo:rustc-env=MCPM_BUILD_VERSION={version}");
    println!("cargo:rustc-env=MCPM_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=MCPM_BUILD_CHANNEL={channel}");
    println!("cargo:rustc-env=MCPM_BUILD_PROFILE={profile}");
    Ok(())
}

fn revision(source_dir: &Path) -> Result<Revision> {
    let commit = git(source_dir, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    let commit = commit.trim();
    ensure!(
        commit.len() >= 6 && commit.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Git returned an invalid commit ID"
    );
    let status = git(
        source_dir,
        &["status", "--porcelain", "--untracked-files=no"],
    )?;
    Ok(Revision {
        commit: commit.to_owned(),
        modified: !status.is_empty(),
    })
}

/// The Git inputs used together for the version suffix and detailed commit identity.
struct Revision {
    commit: String,
    modified: bool,
}

fn contributors(source_dir: &Path) -> Result<String> {
    git(source_dir, &["shortlog", "-sne", "--no-merges", "HEAD"])
}

/// Watch tracked files across the repository, including backend and workflow changes.
fn watch_git(source_dir: &Path) -> Result<()> {
    let paths = git(
        source_dir,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-path",
            "HEAD",
            "--git-path",
            "refs",
            "--git-path",
            "packed-refs",
            "--git-path",
            "shallow",
            "--git-path",
            "index",
        ],
    )?;
    for (index, path) in paths.lines().enumerate() {
        let path = if index == 0 {
            Path::new(path).join(".mailmap")
        } else {
            PathBuf::from(path)
        };
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    let root = paths
        .lines()
        .next()
        .context("missing Git repository root")?;
    let root = Path::new(root);
    let files = git(root, &["ls-files", "--cached", "-z"])?;
    for file in files.split('\0').filter(|file| !file.is_empty()) {
        println!("cargo:rerun-if-changed={}", root.join(file).display());
    }
    Ok(())
}

fn git(source_dir: &Path, args: &[&str]) -> Result<String> {
    let command = format!("git {}", args.join(" "));
    let output = Command::new("git")
        .current_dir(source_dir)
        // Reading status must not update the index and invalidate our own cache.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args)
        .output()
        .with_context(|| format!("failed to run {command}"))?;
    ensure!(
        output.status.success(),
        "{command} exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    String::from_utf8(output.stdout).with_context(|| format!("{command} returned invalid UTF-8"))
}
