//! `workspace.remote.validate`: preflights caller-side Remote workspace registration.
//!
//! This surface operation checks the placeholder's directory, marker, registration file and
//! writable lock before host initialization starts. The final register operation repeats every
//! check under its lock because preflight cannot eliminate races.

use std::{future::Future, path::PathBuf};

use anyhow::Context as _;
use mcpm_exec::{Context, Operation, Placement};
use serde::{Deserialize, Serialize};

use crate::data::workspace::RegistrationWriter;
use crate::env::SurfaceEnv;

/// Preflights one caller-side Remote workspace registration.
#[derive(Debug)]
pub struct Validate;

crate::surface_operation!(Validate, "workspace.remote.validate");

impl Operation for Validate {
    const ID: &'static str = <Self as crate::register::SurfaceOperation>::ID;
    const PLACEMENT: Placement = Placement::Surface;
    type Env = SurfaceEnv;
    type Input = ValidateInput;
    type Event = ();
    type Output = ValidateOutput;
    type Error = ValidateError;

    fn run(
        input: ValidateInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        async move {
            cx.blocking(move || validate(input))
                .await
                .map_err(ValidateError)
        }
    }
}

/// Caller-side registration preflight input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidateInput {
    pub path: PathBuf,
    #[serde(default)]
    pub replace: bool,
}

/// Normalized caller path that passed the preflight.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidateOutput {
    pub path: PathBuf,
}

fn validate(input: ValidateInput) -> Result<ValidateOutput, anyhow::Error> {
    let path = std::fs::canonicalize(&input.path)
        .context("Remote workspace PATH must be an existing local directory")?;
    RegistrationWriter::validate_remote(&path, input.replace)?;
    Ok(ValidateOutput { path })
}

/// A caller-side Remote registration preflight failed.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ValidateError(#[from] pub anyhow::Error);
