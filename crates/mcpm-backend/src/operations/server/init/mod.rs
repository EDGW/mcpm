//! Read-only recognition and complete-data initialization as host operations.

use anyhow::Result;
use mcpm_exec::operation::{Operation, StreamingOutput};
use serde::{Deserialize, Serialize};

mod model;
pub use model::*;
pub mod git;
mod implementation;
pub mod prepare_remote;

pub struct RecognizeServer;
impl Operation for RecognizeServer {
    const ID: &'static str = "server.init.recognize";
    type Input = Recognize;
    type Output = StreamingOutput<RecognizeEvent>;
    async fn run(input: Recognize) -> Result<Self::Output> {
        Ok(StreamingOutput::new(move |sender| async move {
            let progress = sender.clone();
            let runtime = tokio::runtime::Handle::current();
            let candidates = tokio::task::spawn_blocking(move || {
                implementation::recognize(
                    input,
                    || Ok(()),
                    |phase| {
                        runtime.block_on(progress.send(RecognizeEvent::Stage(phase)))?;
                        Ok(())
                    },
                )
            })
            .await??;
            sender.send(RecognizeEvent::Candidates(candidates)).await?;
            Ok(())
        }))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum RecognizeEvent {
    Stage(Phase),
    Candidates(Vec<Candidate>),
}

pub struct Init;
impl Operation for Init {
    const ID: &'static str = "server.init";
    type Input = Initialize;
    type Output = StreamingOutput<InitEvent>;
    async fn run(input: Initialize) -> Result<Self::Output> {
        Ok(StreamingOutput::new(move |sender| async move {
            let progress = sender.clone();
            let runtime = tokio::runtime::Handle::current();
            let result = tokio::task::spawn_blocking(move || {
                implementation::initialize(
                    input,
                    || Ok(()),
                    |phase| {
                        runtime.block_on(progress.send(InitEvent::Stage(phase)))?;
                        Ok(())
                    },
                )
            })
            .await??;
            sender.send(InitEvent::Initialized(result)).await?;
            Ok(())
        }))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum InitEvent {
    Stage(Phase),
    Initialized(Initialized),
}
