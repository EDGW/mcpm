//! Verify the running mcpm installation against its release manifest.

use std::{
    collections::HashSet,
    env,
    fs::{self, File},
    io::{self, Write},
    path::Path,
};

use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use mcpm_exec::error::CombinedError;
use mcpm_lib::download::{Checksum, ChecksumAlgorithm};
use serde::Deserialize;

use crate::display::style::OutputStyle;

const NONSTANDARD: &str =
    "not a standard mcpm installation directory; this may be a debug or modified environment";

#[derive(Args, Debug)]
pub(super) struct ValidateArgs {
    /// Also reject any bin entry not listed in the release manifest.
    #[arg(long)]
    exact: bool,
}

pub(super) fn run(args: ValidateArgs) -> Result<()> {
    let executable = env::current_exe().context("could not locate the running mcpm executable")?;
    let executable = executable
        .canonicalize()
        .context("could not resolve the running mcpm executable")?;
    let bin = executable
        .parent()
        .filter(|dir| dir.file_name().is_some_and(|name| name == "bin"))
        .with_context(|| {
            format!(
                "{NONSTANDARD}: expected <installation>/bin, found {}",
                executable.display()
            )
        })?;
    let root = bin.parent().context(NONSTANDARD)?;
    let path = root.join("manifest.json");
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => bail!("{NONSTANDARD}: {} is not a regular file", path.display()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            bail!("{NONSTANDARD}: missing {}", path.display());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("could not inspect {}", path.display()));
        }
    }

    let bytes = fs::read(&path).with_context(|| format!("could not read {}", path.display()))?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).with_context(|| format!("invalid {}", path.display()))?;
    let mut stdout = io::stdout().lock();
    manifest.verify(bin, &executable, args.exact, &mut stdout)?;

    writeln!(
        stdout,
        "{} {} files in {}.",
        OutputStyle::new().success("Validated"),
        manifest.files.len(),
        root.display()
    )?;
    Ok(())
}

#[derive(Deserialize)]
struct Manifest {
    format: u64,
    files: Vec<Entry>,
}

impl Manifest {
    fn verify(
        &self,
        bin: &Path,
        executable: &Path,
        exact: bool,
        output: &mut impl Write,
    ) -> Result<()> {
        ensure!(
            self.format == 1,
            "unsupported manifest.json format: {}",
            self.format
        );
        let mut names = HashSet::with_capacity(self.files.len());
        let mut entries = Vec::with_capacity(self.files.len());
        for entry in &self.files {
            let name = entry.name()?;
            ensure!(
                names.insert(name),
                "duplicate manifest path: {:?}",
                entry.path
            );
            let checksum = Checksum::new(ChecksumAlgorithm::Sha256, &entry.sha256)
                .with_context(|| format!("invalid SHA-256 in manifest for {:?}", entry.path))?;
            entries.push((name, checksum));
        }
        ensure!(
            executable
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| names.contains(name)),
            "manifest.json does not list the running executable: {}",
            executable.display()
        );
        if exact {
            for entry in fs::read_dir(bin).context("could not list installation bin directory")? {
                let name = entry?.file_name();
                ensure!(
                    name.to_str().is_some_and(|name| names.contains(name)),
                    "bin entry is not listed in manifest.json: {name:?}"
                );
            }
        }

        let mut failures = Vec::new();
        for (name, checksum) in entries {
            writeln!(output, "Validating {name}...")?;
            output.flush()?;
            let path = bin.join(name);
            if let Err(error) = verify_file(&path, &checksum)
                .with_context(|| format!("could not validate {}", path.display()))
            {
                failures.push(error);
            }
        }
        let mut errors = failures.into_iter();
        if let Some(first) = errors.next() {
            if let Some(second) = errors.next() {
                let mut combined = CombinedError::new(first, second);
                for error in errors {
                    combined.push(error);
                }
                return Err(combined).context("installation validation failed");
            }
            return Err(first);
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct Entry {
    path: String,
    sha256: String,
}

impl Entry {
    fn name(&self) -> Result<&str> {
        let name = self.path.strip_prefix("bin/").unwrap_or("");
        ensure!(
            name.as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
                && !name.ends_with('.')
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
            "invalid manifest path {:?}: expected bin/<filename>",
            self.path
        );
        Ok(name)
    }
}

fn verify_file(path: &Path, checksum: &Checksum) -> Result<()> {
    let metadata = fs::symlink_metadata(path).context("missing or inaccessible file")?;
    ensure!(metadata.is_file(), "expected a regular file, not a symlink or directory");
    checksum.verify_reader(&mut File::open(path)?)
}
