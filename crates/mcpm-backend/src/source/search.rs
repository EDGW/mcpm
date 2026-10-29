//! `source.search`: one provider search, always in the calling process.
//!
//! The input carries the source specification (and therefore any credential) and the
//! query; the provider is built from [`SurfaceEnv`] and never leaves this process.

use std::future::Future;

use kako_mc_lib::source::{SearchPage, SearchQuery};
use mcpm_exec::{Context, Operation, Placement};

use crate::env::SurfaceEnv;

/// Searches one remote source.
#[derive(Debug)]
pub struct Search;

crate::surface_operation!(Search, "source.search");

impl Operation for Search {
    const ID: &'static str = <Self as crate::register::SurfaceOperation>::ID;
    const PLACEMENT: Placement = Placement::Surface;
    type Env = SurfaceEnv;
    type Input = SearchInput;
    type Event = ();
    type Output = SearchPage;
    type Error = SearchError;

    fn run(
        input: SearchInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        async move {
            let source = cx.env().source(&input.specification).map_err(SearchError)?;
            tracing::debug!(
                source = input.specification.split('@').next().unwrap_or_default(),
                "Searching remote source"
            );
            source.search(&input.query).await.map_err(|error| {
                SearchError(error.context("mod source search failed"))
            })
        }
    }
}

/// One provider search request.
#[derive(Debug)]
pub struct SearchInput {
    /// Source specification, possibly including a credential.
    pub specification: String,
    pub query: SearchQuery,
}

/// A source search failed; the chain is kept for local rendering.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct SearchError(#[from] pub anyhow::Error);
