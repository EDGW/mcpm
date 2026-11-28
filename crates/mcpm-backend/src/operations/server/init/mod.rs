//! Read-only recognition and complete-data initialization as host operations.

use anyhow::Result;
use mcpm_exec::operation::{Operation, StreamingOutput};
use mcpm_lib::local::mc::server::identify::SkippedStatement;
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
                    |event| {
                        runtime.block_on(progress.send(event))?;
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
    Warning(SkippedStatement),
    Candidates(Vec<Candidate>),
}

pub struct ProbeInitRcon;
impl Operation for ProbeInitRcon {
    const ID: &'static str = "server.init.probe_rcon";
    type Input = ProbeRcon;
    type Output = InitRconProbe;
    async fn run(input: ProbeRcon) -> Result<Self::Output> {
        tokio::task::spawn_blocking(move || implementation::probe_rcon(input)).await?
    }
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
