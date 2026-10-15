use std::{
    fs::{self, File},
    io::Read,
};

use sha1::{Digest, Sha1};

use super::{
    CheckIssue, CheckOptionValue, CheckProblem, CheckReport, CheckedFile, FileCheckStatus,
    PlannedFile, progress::Progress,
};

pub(super) fn check_file(
    report: &mut CheckReport,
    planned: PlannedFile,
    progress: &mut Progress<'_>,
) -> FileCheckStatus {
    let PlannedFile {
        target,
        path,
        size,
        checksums,
        mode,
        ..
    } = planned;
    let status = match verify(&path, size, &checksums, mode) {
        Ok(status) => status,
        Err(problem) => {
            report.issues.push(CheckIssue {
                target: target.clone(),
                path: Some(path.clone()),
                problem,
            });
            FileCheckStatus::Failed
        }
    };
    report.files.push(CheckedFile {
        target,
        path,
        status,
        mode,
    });
    progress.complete_file(status, report);
    status
}

fn verify(
    path: &std::path::Path,
    size: Option<u64>,
    checksums: &[String],
    mode: CheckOptionValue,
) -> Result<FileCheckStatus, CheckProblem> {
    let io_problem = |source: std::io::Error| {
        if source.kind() == std::io::ErrorKind::NotFound {
            CheckProblem::MissingFile
        } else {
            CheckProblem::Io { source }
        }
    };
    let metadata = fs::metadata(path).map_err(io_problem)?;
    if !metadata.is_file() {
        return Err(CheckProblem::NotAFile);
    }
    if let Some(expected) = size
        && expected != metadata.len()
    {
        return Err(CheckProblem::SizeMismatch {
            expected,
            actual: metadata.len(),
        });
    }
    match mode {
        CheckOptionValue::Size => {
            return Ok(if size.is_some() {
                FileCheckStatus::Verified
            } else {
                FileCheckStatus::Unverified
            });
        }
        CheckOptionValue::Skip => return Ok(FileCheckStatus::Unverified),
        CheckOptionValue::Hash => {}
    }
    if checksums.is_empty() {
        return Ok(FileCheckStatus::Unverified);
    }
    for hash in checksums {
        if !super::super::path::sha1(hash) {
            return Err(CheckProblem::InvalidChecksum {
                value: hash.clone(),
            });
        }
    }
    let mut file = File::open(path).map_err(io_problem)?;
    let mut digest = Sha1::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|source| CheckProblem::Io { source })?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    let actual = format!("{:x}", digest.finalize());
    if !checksums
        .iter()
        .any(|expected| actual.eq_ignore_ascii_case(expected))
    {
        return Err(CheckProblem::HashMismatch {
            expected: checksums.to_vec(),
            actual,
        });
    }
    Ok(FileCheckStatus::Verified)
}
