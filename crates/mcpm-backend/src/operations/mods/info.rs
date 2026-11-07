//! Host Mod metadata inspection over the shared selection and loading pipeline.

use super::loading::inspection::{self, Event, Input};
use anyhow::Result;
use mcpm_exec::operation::{Operation, StreamingOutput};

pub struct Info;
impl Operation for Info {
    const ID: &'static str = "mod.info";
    type Input = Input;
    type Output = StreamingOutput<Event>;
    async fn run(input: Input) -> Result<Self::Output> {
        inspection::start(input).await
    }
}
