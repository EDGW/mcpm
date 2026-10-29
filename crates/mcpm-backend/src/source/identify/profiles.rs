//! `source.profiles`: cache identities and summary requirements of configured sources.
//!
//! Preparation only: no request is made and nothing is displayed. The digest needs the
//! fingerprint requirements before any summary exists, and a later identification write
//! needs the stable cache identity; both come from the same provider construction the
//! identify operation uses. A specification that cannot be built is reported in its
//! profile, so the identify operation still fails only that source's items.

use std::future::Future;

use kako_mc_lib::error::ErrorReport;
use kako_mc_lib::source::SummaryRequirements;
use mcpm_exec::{Context, Operation, Placement};

use crate::env::SurfaceEnv;
use crate::source::identify::SourceProfile;

/// Reports source cache identities and digest requirements without contacting any source.
#[derive(Debug)]
pub struct Profiles;

crate::surface_operation!(Profiles, "source.profiles");

impl Operation for Profiles {
    const ID: &'static str = <Self as crate::register::SurfaceOperation>::ID;
    const PLACEMENT: Placement = Placement::Surface;
    type Env = SurfaceEnv;
    type Input = ProfilesInput;
    type Event = ();
    type Output = ProfilesOutput;
    type Error = ProfilesError;

    fn run(
        input: ProfilesInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        async move {
            let profiles = input
                .specifications
                .iter()
                .map(|specification| {
                    let cache_key = crate::source::provider::family(specification).to_owned();
                    match cx.env().source(specification) {
                        Ok(source) => SourceProfile {
                            cache_key: source.identification_cache_id().0,
                            requirements: source.summary_requirements(),
                            error: None,
                        },
                        Err(error) => SourceProfile {
                            cache_key,
                            requirements: SummaryRequirements::default(),
                            error: Some(ErrorReport::capture(&error)),
                        },
                    }
                })
                .collect();
            Ok(ProfilesOutput { profiles })
        }
    }
}

/// Source specifications in output order, each possibly carrying a credential.
#[derive(Debug)]
pub struct ProfilesInput {
    pub specifications: Vec<String>,
}

/// One profile per specification, in request order.
#[derive(Debug)]
pub struct ProfilesOutput {
    pub profiles: Vec<SourceProfile>,
}

/// Preparation could not run; per-source problems are part of each profile.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ProfilesError(#[from] pub anyhow::Error);
