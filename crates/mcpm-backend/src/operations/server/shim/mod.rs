//! Short Host operations composing launch preparation and independent shim control.

use anyhow::Result;
use mcpm_exec::operation::{Operation, StreamingOutput};
use mcpm_lib::{
    download::{self, DownloadConfig, sources},
    local::mc::server::{authlib_injector, launch},
};

mod model;
use crate::server::shim::{self, Request};
pub use model::*;

pub struct Prepare;
impl Operation for Prepare {
    const ID: &'static str = "server.launch.prepare";
    type Input = PrepareInput;
    type Output = StreamingOutput<PrepareEvent>;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        Ok(StreamingOutput::new(move |sender| async move {
            let server = std::fs::canonicalize(&input.server)?;
            crate::workspace::require_local(&server)?;
            let (updates, mut progress) =
                tokio::sync::watch::channel::<Option<(String, Option<(u64, Option<u64>)>)>>(None);
            let observe = |event: launch::Event<'_>| {
                sender.checkpoint()?;
                let (message, bytes) = match event {
                    launch::Event::Stage(stage) => (
                        match stage {
                            launch::Stage::Configuration => "Reading launch configuration",
                            launch::Stage::LocalChecks => "Checking Java and launch arguments",
                            launch::Stage::JavaCapabilities => {
                                "Checking Java argument-file support"
                            }
                            launch::Stage::Ready => "Launch preparation complete",
                        }
                        .to_owned(),
                        None,
                    ),
                    launch::Event::Authentication(event) => match event {
                        authlib_injector::Event::Checking => {
                            ("Checking authlib-injector".into(), None)
                        }
                        authlib_injector::Event::Resolving => {
                            ("Resolving authlib-injector release".into(), None)
                        }
                        authlib_injector::Event::Installing => {
                            ("Installing authlib-injector".into(), None)
                        }
                        authlib_injector::Event::Ready => ("authlib-injector ready".into(), None),
                        authlib_injector::Event::Download(event) => match event {
                            download::Event::AttemptStarted { kind, source } => match kind {
                                download::RequestKind::Metadata => (
                                    format!("Resolving authlib-injector release · {source}"),
                                    None,
                                ),
                                download::RequestKind::File => (
                                    format!("Downloading authlib-injector · {source}"),
                                    Some((0, None)),
                                ),
                            },
                            download::Event::Bytes { received, total } => (
                                updates
                                    .borrow()
                                    .as_ref()
                                    .map(|(message, _)| message.clone())
                                    .unwrap_or_else(|| "Downloading authlib-injector".into()),
                                Some((received, total)),
                            ),
                            download::Event::Verifying => {
                                ("Verifying authlib-injector download".into(), None)
                            }
                            download::Event::AttemptFailed { source, .. } => {
                                (format!("Download attempt failed · {source}"), None)
                            }
                            download::Event::Completed { .. } => ("Download complete".into(), None),
                        },
                    },
                };
                updates.send_replace(Some((message, bytes)));
                Ok(())
            };
            let prepare = launch::prepare_with_progress(
                &server,
                &input.overrides,
                input.auth_jar_policy,
                || sources::downloader(&DownloadConfig::default()),
                &observe,
            );
            tokio::pin!(prepare);
            let plan = loop {
                tokio::select! {
                    result = &mut prepare => break result?,
                    changed = progress.changed() => {
                        changed?;
                        let event = progress.borrow_and_update().clone();
                        if let Some((message, bytes)) = event { sender.send(PrepareEvent::Progress { message, bytes }).await?; }
                    }
                }
            };
            sender
                .send(PrepareEvent::Prepared(Prepared {
                    server: server.clone(),
                    plan: EncodedPlan::encode(&plan)?,
                }))
                .await?;
            Ok(())
        }))
    }
}

pub struct Launch;
impl Operation for Launch {
    const ID: &'static str = "server.shim.launch";
    type Input = LaunchInput;
    type Output = Launched;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        shim::launch(input).await
    }
}

macro_rules! forwarding_operation {
    ($name:ident, $id:literal, $input:ty, $output:ty, $variant:ident) => {
        pub struct $name;
        impl Operation for $name {
            const ID: &'static str = $id;
            type Input = $input;
            type Output = $output;
            async fn run(input: Self::Input) -> Result<Self::Output> {
                shim::call(Request::$variant(input))
                    .await
                    .map(|(output, _)| output)
            }
        }
    };
}
pub struct Inspect;
impl Operation for Inspect {
    const ID: &'static str = "server.shim.inspect";
    type Input = InspectInput;
    type Output = Snapshot;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        let (mut snapshot, server) = shim::call::<Snapshot>(Request::Inspect(input)).await?;
        if let Some(server) = server {
            snapshot.reference.server = server;
        }
        Ok(snapshot)
    }
}

pub struct SubscribeStatus;
impl Operation for SubscribeStatus {
    const ID: &'static str = "server.shim.status";
    type Input = StatusInput;
    type Output = StreamingOutput<Snapshot>;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        Ok(StreamingOutput::new(move |sender| async move {
            shim::subscribe_status(input, sender).await
        }))
    }
}
forwarding_operation!(
    Command,
    "server.shim.command",
    CommandInput,
    CommandResult,
    Command
);
pub struct SubscribeLogs;
impl Operation for SubscribeLogs {
    const ID: &'static str = "server.shim.logs";
    type Input = LogsInput;
    type Output = StreamingOutput<LogBatch>;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        Ok(StreamingOutput::new(move |sender| async move {
            shim::subscribe_logs(input, sender).await
        }))
    }
}
pub struct SubscribeCommands;
impl Operation for SubscribeCommands {
    const ID: &'static str = "server.shim.commands";
    type Input = CommandsInput;
    type Output = StreamingOutput<CommandEvent>;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        Ok(StreamingOutput::new(move |sender| async move {
            shim::subscribe_commands(input, sender).await
        }))
    }
}
forwarding_operation!(Kill, "server.shim.kill", ControlInput, KillResult, Kill);
