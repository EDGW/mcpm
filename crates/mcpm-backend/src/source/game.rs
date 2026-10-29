//! `source.game.versions`: the selected provider's Minecraft version catalog.
//!
//! Only surface execution can build a provider, so the catalog request stays in the
//! calling process; TUI state keeps the returned list and never holds a provider.

use mcpm_exec::{Context, Operation, Placement};

use crate::env::SurfaceEnv;

/// Fetches one provider's Minecraft version catalog.
#[derive(Debug)]
pub struct GameVersions;

crate::surface_operation!(GameVersions, "source.game.versions");

impl Operation for GameVersions {
    const ID: &'static str = <Self as crate::register::SurfaceOperation>::ID;
    const PLACEMENT: Placement = Placement::Surface;
    type Env = SurfaceEnv;
    type Input = GameVersionsInput;
    type Event = ();
    type Output = Vec<String>;
    type Error = GameVersionsError;

    fn run(
        input: GameVersionsInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        async move {
            let source = cx
                .env()
                .source(&input.specification)
                .map_err(GameVersionsError)?;
            source
                .game_versions()
                .await
                .map_err(|error| GameVersionsError(error.context("failed to load game versions")))
        }
    }
}

/// One provider specification.
#[derive(Debug)]
pub struct GameVersionsInput {
    pub specification: String,
}

/// A catalog request failed; the chain is kept for local rendering.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct GameVersionsError(#[from] pub anyhow::Error);
