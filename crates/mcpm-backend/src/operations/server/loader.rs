//! Host-side streaming orchestration for in-place mod loader installation (`server.loader.add`)
//! and shared loader installation helpers reused by `server.new`.
//!
//! Coordinates read-only preflight (with pending-journal recovery on detection), upstream
//! installer resolution, incremental `.mcpm/.gitignore` updates, isolated staging execution
//! with pre-commit cancellation, exclusive lock publication via [`loader_install::publish_locked`],
//! and post-commit [`ServerInfo`] inspection without creating an empty `mods/` directory.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result, bail};
use mcpm_exec::operation::{Operation, StreamSender, StreamingOutput};
use mcpm_lib::{
    download::{
        self, DownloadConfig, Downloader, MappingTable,
        loader::{self, LoaderFamily},
        sources,
    },
    local::{
        codec::Codec,
        mc::server::{
            MinecraftServer, MinecraftServerError, config, launch,
            loader_install::{
                self, InstallRequest, InstallerFailed, MirrorAgentSpec, PendingPublication,
                PreflightSnapshot, StagingDir,
            },
        },
    },
};
use serde::{Deserialize, Serialize};

use super::info::{ServerInfo, StartCommand};
use crate::{server::shim, workspace};

/// Streaming operation converting an existing v4 Vanilla server into a Modded server in place.
pub struct AddLoader;

impl Operation for AddLoader {
    const ID: &'static str = "server.loader.add";
    type Input = AddLoaderInput;
    type Output = StreamingOutput<AddLoaderEvent>;

    async fn run(input: AddLoaderInput) -> Result<Self::Output> {
        Ok(StreamingOutput::new(move |sender| async move {
            let download_config = DownloadConfig::decode(&input.download_config)?;
            let downloader = sources::downloader(&download_config)?;
            let server_info = install_on_existing_server(
                input.path,
                input.family,
                input.loader_version,
                input.force,
                &downloader,
                None,
                &sender,
                Some,
            )
            .await?;
            let _ = sender
                .send(AddLoaderEvent::Completed(Box::new(server_info)))
                .await;
            Ok(())
        }))
    }
}

/// Request payload for `server.loader.add`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddLoaderInput {
    pub path: PathBuf,
    pub family: LoaderFamily,
    #[serde(default)]
    pub loader_version: Option<String>,
    pub download_config: Vec<u8>,
    #[serde(default)]
    pub force: bool,
}

/// Loader family and optional requested version shared with `server.new`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoaderRequest {
    pub family: LoaderFamily,
    #[serde(default)]
    pub version: Option<String>,
}

/// Resolved canonical loader target frozen during `server.new` preparation before any disk write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreparedLoader {
    pub family: LoaderFamily,
    pub loader_version: String,
}

/// Stable streaming event contract for direct and mirror-injected installer execution.
#[derive(Debug, Serialize, Deserialize)]
pub enum AddLoaderEvent {
    Stage {
        stage: AddLoaderStage,
        message: String,
    },
    DownloadProgress {
        message: String,
        received: u64,
        total: Option<u64>,
    },
    InstallerAttempt {
        source: String,
        attempt: usize,
    },
    Notice(String),
    Completed(Box<ServerInfo>),
}

/// High-level execution stages reported over [`AddLoaderEvent::Stage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AddLoaderStage {
    Preflight,
    Resolve,
    Download,
    Install,
    Verify,
    Publish,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallerSourcePlan {
    source_id: String,
    table: MappingTable,
    notice: Option<String>,
}

impl InstallerSourcePlan {
    fn uses_mirror_agent(&self) -> bool {
        !self.table.is_empty()
    }
}

/// Ensures the blocking installer cancellation flag is flipped synchronously if the async
/// operation future is aborted or dropped during runtime shutdown before reaching Stage 5.
struct CancelOnDrop(Option<Arc<AtomicBool>>);

impl CancelOnDrop {
    fn new(flag: Arc<AtomicBool>) -> Self {
        Self(Some(flag))
    }

    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(flag) = self.0.take() {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

/// Validated installer source selection and optional verified Host mirror agent path.
pub(super) struct PreparedInstallerPlan {
    source_plan: InstallerSourcePlan,
    host_agent_jar: Option<PathBuf>,
}

/// Validates installer source compatibility and bundled mirror-agent integrity (when needed)
/// on a blocking thread before doing any network or disk work.
pub(super) async fn prepare_installer_plan(
    downloader: &Downloader,
) -> Result<PreparedInstallerPlan> {
    let source_plan = select_installer_source(downloader)?;
    let host_agent_jar = if source_plan.uses_mirror_agent() {
        Some(tokio::task::spawn_blocking(crate::artifacts::loader_injector_agent).await??)
    } else {
        None
    };
    Ok(PreparedInstallerPlan {
        source_plan,
        host_agent_jar,
    })
}

/// Resolves a fixed canonical loader version for `minecraft_version` without writing to disk
/// (reused by `server.new` preparation after [`prepare_installer_plan`] succeeds).
pub(super) async fn prepare_for_new<E, F>(
    downloader: &Downloader,
    minecraft_version: &str,
    request: &LoaderRequest,
    sender: &StreamSender<E>,
    map_event: F,
) -> Result<PreparedLoader>
where
    E: Send + 'static,
    F: Fn(AddLoaderEvent) -> Option<E> + Send + Sync + Clone + 'static,
{
    send_mapped(
        sender,
        &map_event,
        AddLoaderEvent::Stage {
            stage: AddLoaderStage::Resolve,
            message: format!(
                "Resolving {} installer for Minecraft {minecraft_version}...",
                request.family
            ),
        },
    )
    .await?;

    let (updates, mut receiver) = tokio::sync::watch::channel::<Option<ProgressUpdate>>(None);
    let checkpoint = sender.clone();
    let observe = move |event: download::Event<'_>| {
        checkpoint.checkpoint()?;
        updates.send_replace(Some(map_download_event(
            event,
            "Resolving installer metadata",
        )));
        Ok(())
    };
    let release = drive_with_progress(
        sender,
        &map_event,
        AddLoaderStage::Resolve,
        &mut receiver,
        loader::resolve_with_progress(
            downloader,
            request.family,
            minecraft_version,
            request.version.as_deref(),
            &observe,
        ),
    )
    .await?;

    Ok(PreparedLoader {
        family: release.family(),
        loader_version: release.loader_version().to_owned(),
    })
}

/// Executes the complete 6-stage loader installation pipeline onto an existing v4 Vanilla
/// server directory, forwarding progress and notices through `map_event`.
#[allow(clippy::too_many_arguments)]
pub(super) async fn install_on_existing_server<E, F>(
    path: PathBuf,
    family: LoaderFamily,
    loader_version: Option<String>,
    force: bool,
    downloader: &Downloader,
    prepared_plan: Option<PreparedInstallerPlan>,
    sender: &StreamSender<E>,
    map_event: F,
) -> Result<ServerInfo>
where
    E: Send + 'static,
    F: Fn(AddLoaderEvent) -> Option<E> + Send + Sync + Clone + 'static,
{
    // Stage 1: strictly read-only preflight (zero disk writes unless recovering a pending journal).
    send_mapped(
        sender,
        &map_event,
        AddLoaderEvent::Stage {
            stage: AddLoaderStage::Preflight,
            message: "Validating server...".to_owned(),
        },
    )
    .await?;

    let (source_plan, host_agent_jar, snapshot) = match prepared_plan {
        Some(plan) => {
            let snapshot = tokio::task::spawn_blocking(move || preflight_host(&path)).await??;
            (plan.source_plan, plan.host_agent_jar, snapshot)
        }
        None => {
            let source_plan = select_installer_source(downloader)?;
            let needs_agent = source_plan.uses_mirror_agent();
            let (host_agent_jar, snapshot) =
                tokio::task::spawn_blocking(move || -> Result<(Option<PathBuf>, PreflightSnapshot)> {
                    let agent = if needs_agent {
                        Some(crate::artifacts::loader_injector_agent()?)
                    } else {
                        None
                    };
                    let snapshot = preflight_host(&path)?;
                    Ok((agent, snapshot))
                })
                .await??;
            (source_plan, host_agent_jar, snapshot)
        }
    };

    // Stage 2: resolve fixed InstallerRelease upstream (still zero disk writes).
    send_mapped(
        sender,
        &map_event,
        AddLoaderEvent::Stage {
            stage: AddLoaderStage::Resolve,
            message: format!(
                "Resolving {family} installer for Minecraft {}...",
                snapshot.minecraft_version
            ),
        },
    )
    .await?;

    let release = {
        let (updates, mut receiver) = tokio::sync::watch::channel::<Option<ProgressUpdate>>(None);
        let checkpoint = sender.clone();
        let observe = move |event: download::Event<'_>| {
            checkpoint.checkpoint()?;
            updates.send_replace(Some(map_download_event(
                event,
                "Resolving installer metadata",
            )));
            Ok(())
        };
        drive_with_progress(
            sender,
            &map_event,
            AddLoaderStage::Resolve,
            &mut receiver,
            loader::resolve_with_progress(
                downloader,
                family,
                &snapshot.minecraft_version,
                loader_version.as_deref(),
                &observe,
            ),
        )
        .await?
    };

    // Stage 3: incremental .mcpm/.gitignore + isolated staging + installer download + execution.
    sender.checkpoint()?;
    let (staging, mirror_agent) = {
        let root = snapshot.root.clone();
        let wd_rel = snapshot.working_directory_rel.clone();
        let table = source_plan.table;
        tokio::task::spawn_blocking(move || -> Result<(StagingDir, Option<MirrorAgentSpec>)> {
            workspace::ensure_gitignore(&root)?;
            let staging = StagingDir::create(&root, &wd_rel)?;
            let mirror_agent = match host_agent_jar {
                Some(agent_source) => {
                    let agent_jar = staging.stage_agent_jar(&agent_source)?;
                    Some(MirrorAgentSpec { agent_jar, table })
                }
                None => None,
            };
            Ok((staging, mirror_agent))
        })
        .await??
    };

    let installer_label = format!(
        "{} {} installer",
        release.family(),
        release.loader_version()
    );
    if !release.has_checksum() {
        send_mapped(
            sender,
            &map_event,
            AddLoaderEvent::Notice(format!(
                "Could not fetch SHA-1 checksum for {installer_label} from the configured download sources; continuing without checksum verification."
            )),
        )
        .await?;
    }
    send_mapped(
        sender,
        &map_event,
        AddLoaderEvent::Stage {
            stage: AddLoaderStage::Download,
            message: format!("Downloading {installer_label}..."),
        },
    )
    .await?;

    let installer_jar = staging.installer_jar_path().to_path_buf();
    {
        let (updates, mut receiver) = tokio::sync::watch::channel::<Option<ProgressUpdate>>(None);
        let checkpoint = sender.clone();
        let download_label = format!("Downloading {installer_label}");
        let observe = move |event: download::Event<'_>| {
            checkpoint.checkpoint()?;
            updates.send_replace(Some(map_download_event(event, &download_label)));
            Ok(())
        };
        drive_with_progress(
            sender,
            &map_event,
            AddLoaderStage::Download,
            &mut receiver,
            release.download_to_with_progress(downloader, &installer_jar, &observe),
        )
        .await?;
    }

    if let Some(notice) = source_plan.notice {
        send_mapped(sender, &map_event, AddLoaderEvent::Notice(notice)).await?;
    }

    let source_id = source_plan.source_id;
    send_mapped(
        sender,
        &map_event,
        AddLoaderEvent::InstallerAttempt {
            source: source_id.clone(),
            attempt: 1,
        },
    )
    .await?;
    send_mapped(
        sender,
        &map_event,
        AddLoaderEvent::Stage {
            stage: AddLoaderStage::Install,
            message: format!("Running {installer_label} · {source_id}..."),
        },
    )
    .await?;

    // Pre-commit cancellation watcher + RAII drop guard: kills the installer process tree
    // immediately on stream disconnect OR when Tokio drops this task during local Ctrl-C shutdown.
    let cancel = Arc::new(AtomicBool::new(false));
    let mut cancel_guard = CancelOnDrop::new(cancel.clone());
    let cancel_watcher = tokio::spawn({
        let sender = sender.clone();
        let cancel = cancel.clone();
        async move {
            sender.closed().await;
            cancel.store(true, Ordering::Relaxed);
        }
    });

    let (verify_tx, verify_rx) = tokio::sync::oneshot::channel::<()>();
    let verify_forwarder = tokio::spawn({
        let sender = sender.clone();
        let map_event = map_event.clone();
        async move {
            if verify_rx.await.is_ok() {
                let _ = send_mapped(
                    &sender,
                    &map_event,
                    AddLoaderEvent::Stage {
                        stage: AddLoaderStage::Verify,
                        message: "Verifying staged loader installation...".to_owned(),
                    },
                )
                .await;
            }
        }
    });

    let family = release.family();
    let loader_version = release.loader_version().to_owned();
    let stage_span = tracing::Span::current();
    let stage_result = tokio::task::spawn_blocking(move || {
        let _entered = stage_span.enter();
        let result = loader_install::stage_and_verify_with_hook(
            &snapshot,
            &staging,
            &InstallRequest {
                family,
                loader_version: &loader_version,
                force,
                agent: mirror_agent.as_ref(),
                round: 1,
                cancel: Some(cancel.as_ref()),
            },
            || {
                let _ = verify_tx.send(());
            },
        );
        (snapshot, staging, result)
    })
    .await?;

    cancel_guard.disarm();
    cancel_watcher.abort();
    let _ = verify_forwarder.await;
    let (snapshot, staging, staged_result) = stage_result;
    let staged = staged_result
        .map_err(|error| {
            if error
                .chain()
                .any(|cause| cause.downcast_ref::<InstallerFailed>().is_some())
            {
                error.context(format!(
                    "loader installer failed using download source '{source_id}'"
                ))
            } else {
                error
            }
        })
        .map_err(super::shim::attach_executable_tip)?;

    // Last cancellation check before entering the non-interruptible publication commit window.
    sender.checkpoint()?;
    send_mapped(
        sender,
        &map_event,
        AddLoaderEvent::Stage {
            stage: AddLoaderStage::Publish,
            message: "Publishing loader artifacts and committing configuration...".to_owned(),
        },
    )
    .await?;

    // Stage 5 & 6: exclusive lock commit window + staging cleanup + ServerInfo snapshot.
    // Once entered, stream disconnects are ignored so publication always completes consistently.
    let server_info = tokio::task::spawn_blocking(move || -> Result<ServerInfo> {
        let lock = config::ServerLock::acquire_write(&snapshot.root)
            .context("server is busy or another operation holds the server lock")?;
        shim::ensure_inactive_readonly(&snapshot.root)?;
        loader_install::publish_locked(
            &snapshot.root,
            &lock,
            &snapshot,
            &staged.verified,
            &staging,
            force,
        )?;
        drop(lock);

        if let Err(error) = staging.cleanup() {
            tracing::warn!(
                %error,
                "loader installation committed, but failed to clean up staging directory"
            );
        }

        // Use inspect() (not load()) so no empty `mods/` directory is eagerly created (D4).
        let server = MinecraftServer::inspect(&snapshot.root)
            .map_err(MinecraftServerError::into_diagnostic)?;
        let mut info = ServerInfo::from_server(&server)?;
        info.start = launch::describe(&snapshot.root)?.map(StartCommand::from);
        Ok(info)
    })
    .await??;

    Ok(server_info)
}

async fn send_mapped<E, F>(
    sender: &StreamSender<E>,
    map_event: &F,
    event: AddLoaderEvent,
) -> Result<()>
where
    E: Send + 'static,
    F: Fn(AddLoaderEvent) -> Option<E>,
{
    if let Some(mapped) = map_event(event) {
        sender.send(mapped).await?;
    }
    Ok(())
}

/// Host-side read-only preflight; recovers an interrupted v1 publication journal if present
/// and immediately aborts without proceeding to installation.
fn preflight_host(path: &Path) -> Result<PreflightSnapshot> {
    let root = fs::canonicalize(path)
        .with_context(|| format!("failed to resolve server directory {path:?}"))?;
    workspace::require_local(&root)?;
    shim::ensure_inactive_readonly(&root)?;
    match loader_install::preflight(&root) {
        Ok(snapshot) => Ok(snapshot),
        Err(error)
            if error
                .chain()
                .any(|cause| cause.downcast_ref::<PendingPublication>().is_some()) =>
        {
            let lock = config::ServerLock::acquire_write(&root)
                .context("cannot acquire server lock to recover interrupted publication")?;
            shim::ensure_inactive_readonly(&root)?;
            config::recover_if_pending(&root, &lock)?;
            bail!(
                "recovered an interrupted previous configuration publication; inspect the server files and retry"
            );
        }
        Err(error) => Err(error),
    }
}

/// Selects the first configured source exporting a static [`MappingTable`] (non-empty for
/// mirror injection, empty for direct execution). Sources returning `None` are skipped with a
/// notice; if no configured source supports static mapping or direct execution, fails explicitly.
fn select_installer_source(downloader: &Downloader) -> Result<InstallerSourcePlan> {
    if downloader.sources().is_empty() {
        bail!("no download sources are configured for loader installation");
    }
    let mut skipped_dynamic = Vec::new();
    for source in downloader.sources() {
        if let Some(table) = source.mapping_table()? {
            let notice = (!skipped_dynamic.is_empty()).then(|| {
                format!(
                    "Skipping dynamic download source(s) ({}) without a static prefix mapping table for installer internal downloads; using '{}'.",
                    skipped_dynamic.join(", "),
                    source.id(),
                )
            });
            return Ok(InstallerSourcePlan {
                source_id: source.id().to_owned(),
                table,
                notice,
            });
        }
        skipped_dynamic.push(source.id().to_owned());
    }

    bail!(
        "none of the configured download sources ({}) can provide a static prefix mapping table or direct connection for loader installers",
        skipped_dynamic.join(", ")
    );
}

#[derive(Clone)]
enum ProgressUpdate {
    Stage(String),
    Bytes {
        message: String,
        received: u64,
        total: Option<u64>,
    },
}

async fn drive_with_progress<E, F, T, Fut>(
    sender: &StreamSender<E>,
    map_event: &F,
    stage: AddLoaderStage,
    receiver: &mut tokio::sync::watch::Receiver<Option<ProgressUpdate>>,
    work: Fut,
) -> Result<T>
where
    E: Send + 'static,
    F: Fn(AddLoaderEvent) -> Option<E>,
    Fut: std::future::Future<Output = Result<T>>,
{
    tokio::pin!(work);
    let mut watch_closed = false;
    loop {
        tokio::select! {
            result = &mut work => return result,
            _ = sender.closed() => bail!("loader installation cancelled"),
            changed = receiver.changed(), if !watch_closed => {
                if changed.is_err() {
                    watch_closed = true;
                    continue;
                }
                let update = receiver.borrow_and_update().clone();
                if let Some(update) = update {
                    let event = match update {
                        ProgressUpdate::Stage(message) => AddLoaderEvent::Stage { stage, message },
                        ProgressUpdate::Bytes { message, received, total } => {
                            AddLoaderEvent::DownloadProgress {
                                message,
                                received,
                                total,
                            }
                        }
                    };
                    send_mapped(sender, map_event, event).await?;
                }
            }
        }
    }
}

fn map_download_event(event: download::Event<'_>, file_label: &str) -> ProgressUpdate {
    match event {
        download::Event::AttemptStarted { kind, source } => match kind {
            download::RequestKind::Metadata => {
                ProgressUpdate::Stage(format!("Reading loader metadata · {source}"))
            }
            download::RequestKind::File => ProgressUpdate::Bytes {
                message: format!("{file_label} · {source}"),
                received: 0,
                total: None,
            },
        },
        download::Event::Bytes { received, total } => ProgressUpdate::Bytes {
            message: file_label.to_owned(),
            received,
            total,
        },
        download::Event::Verifying => ProgressUpdate::Stage(format!("Verifying {file_label}...")),
        download::Event::AttemptFailed { source, .. } => {
            ProgressUpdate::Stage(format!("Download attempt failed · {source}"))
        }
        download::Event::Completed { .. } => ProgressUpdate::Stage("Download complete".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use mcpm_lib::{
        download::DownloadOptions,
        local::mc::server::{
            MinecraftServerMetadata, SERVER_METADATA_VERSION, ServerLoader, ServerModLoader,
            config::{self, ConfAction, LaunchAction, Publication, ServerLock},
        },
    };

    use super::*;

    fn write_vanilla_server(root: &Path, loader: ServerLoader) {
        fs::write(root.join("server.jar"), b"placeholder-vanilla-jar").unwrap();

        let draft = config::vanilla_server().unwrap();
        let conf = "version = 1\n\n[java]\n\n[jvm]\n[jvm.memory]\n[jvm.gc]\n\n[server]\nnogui = true\n\n[application]\n";
        let metadata = MinecraftServerMetadata {
            version: SERVER_METADATA_VERSION,
            minecraft_version: "1.21.1".to_owned(),
            loader,
        };
        let lock = ServerLock::acquire_write(root).unwrap();
        config::commit(
            root,
            &lock,
            Publication {
                identity: &metadata,
                launch: LaunchAction::Write(&draft.launch),
                conf: ConfAction::Write(conf),
                replace_existing: true,
                recover_pending: false,
            },
        )
        .unwrap();
    }

    #[test]
    fn installer_source_selection_supports_direct_mirror_and_mixed_lists() {
        let official_only =
            DownloadConfig::new(vec!["official".into()], DownloadOptions::default()).unwrap();
        let plan =
            select_installer_source(&sources::downloader(&official_only).unwrap()).unwrap();
        assert_eq!(plan.source_id, "official");
        assert!(!plan.uses_mirror_agent());
        assert!(plan.notice.is_none());

        let default_mixed = DownloadConfig::default();
        let plan =
            select_installer_source(&sources::downloader(&default_mixed).unwrap()).unwrap();
        assert_eq!(plan.source_id, "bmclapi");
        assert!(plan.uses_mirror_agent());
        assert!(plan.notice.is_none());

        let mirror_only =
            DownloadConfig::new(vec!["bmclapi".into()], DownloadOptions::default()).unwrap();
        let plan = select_installer_source(&sources::downloader(&mirror_only).unwrap()).unwrap();
        assert_eq!(plan.source_id, "bmclapi");
        assert!(plan.uses_mirror_agent());
        assert!(plan.notice.is_none());
    }

    #[tokio::test]
    async fn add_loader_rejects_ineligible_running_and_pending_with_expected_disk_state() {
        // 1. Modded server rejected with zero writes (no .gitignore, no loader-install/, no run/).
        let modded = tempfile::tempdir().unwrap();
        write_vanilla_server(
            modded.path(),
            ServerLoader::Modded(ServerModLoader {
                loader_type: "fabric".into(),
                version: "0.16.10".into(),
            }),
        );
        fs::remove_file(modded.path().join(".mcpm/server.lock")).unwrap();
        let mut stream = AddLoader::run(AddLoaderInput {
            path: modded.path().to_path_buf(),
            family: LoaderFamily::Fabric,
            loader_version: None,
            download_config: DownloadConfig::default().encode().unwrap(),
            force: false,
        })
        .await
        .unwrap();
        let mut saw_error = false;
        while let Some(item) = stream.next().await {
            if item.is_err() {
                saw_error = true;
            }
        }
        assert!(saw_error);
        assert!(!modded.path().join(".mcpm/server.lock").exists());
        assert!(!modded.path().join(".mcpm/.gitignore").exists());
        assert!(!modded.path().join(".mcpm/loader-install").exists());
        assert!(!modded.path().join(".mcpm/run").exists());

        // 2. Active running.lock rejected with zero loader-install writes.
        let running = tempfile::tempdir().unwrap();
        write_vanilla_server(running.path(), ServerLoader::Vanilla);
        let run_lock = shim::acquire_running_lock(running.path(), true).unwrap();
        assert!(preflight_host(running.path()).is_err());
        drop(run_lock);
        assert!(!running.path().join(".mcpm/loader-install").exists());

        // 3. Pending publication journal is recovered and immediately rejected.
        let pending = tempfile::tempdir().unwrap();
        write_vanilla_server(pending.path(), ServerLoader::Vanilla);
        let old_meta = fs::read(pending.path().join(".mcpm/server.json")).unwrap();
        let journal = serde_json::json!({
            "version": 1,
            "files": [
                {
                    "target": "metadata",
                    "before": old_meta,
                    "after": b"corrupt-new-meta".to_vec()
                }
            ]
        });
        fs::write(
            pending.path().join(".mcpm/launch-publication.json"),
            serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();
        fs::write(pending.path().join(".mcpm/server.json"), b"corrupt-new-meta").unwrap();
        let err = preflight_host(pending.path()).unwrap_err();
        assert!(
            format!("{err:#}").contains("recovered an interrupted previous configuration publication"),
            "unexpected error: {err:#}"
        );
        assert!(!pending.path().join(".mcpm/launch-publication.json").exists());
        assert_eq!(
            fs::read(pending.path().join(".mcpm/server.json")).unwrap(),
            old_meta
        );
    }
}
