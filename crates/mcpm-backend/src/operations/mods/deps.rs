//! Host Mod dependency declarations using the same acquisition as metadata inspection.

use super::loading::inspection::{self, Event, Input};
use anyhow::Result;
use mcpm_exec::operation::{Operation, StreamingOutput};

pub struct Deps;
impl Operation for Deps {
    const ID: &'static str = "mod.deps";
    type Input = Input;
    type Output = StreamingOutput<Event>;
    async fn run(input: Input) -> Result<Self::Output> {
        inspection::start(input).await
    }
}
