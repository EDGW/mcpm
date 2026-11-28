//! Host-side vanilla creation followed by the same complete-data setup used by init,
//! with optional immediate mod loader installation via the shared `loader` pipeline.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use mcpm_exec::{
    error::PortableError,
    operation::{Operation, StreamSender, StreamingOutput},
};
use mcpm_lib::{
    download::{self, DownloadConfig, Downloader, loader::LoaderFamily, mojang, sources},
    local::{
        codec::Codec,
        mc::server::{
            MinecraftServerMetadata, SERVER_METADATA_VERSION, ServerLoader, VANILLA_SERVER_JAR,
            config, launch, loader_install::InstallCancelled, properties,
        },
    },
};
use serde::{Deserialize, Serialize};

use super::{
    info::{ServerInfo, StartCommand},
    init::git,
    loader::{self, AddLoaderEvent, AddLoaderStage, LoaderRequest, PreparedLoader},
};
use crate::workspace::initialize::{self, LocalConfiguration, LocalInitialization};

type Update = (String, Option<(u64, Option<u64>)>);

// Mojang releaseTime for 1.9-pre4: 2016-02-26T15:21:11+00:00, inclusive.
const RCON_PROPERTIES_SINCE: i64 = 1_456_500_071;

pub struct New;
impl Operation for New {
    const ID: &'static str = "server.new";
    type Input = NewInput;
    type Output = StreamingOutput<NewEvent>;

    async fn run(input: NewInput) -> Result<Self::Output> {
        Ok(StreamingOutput::new(move |sender| async move {
            let (config_bytes, path) = match &input {
                NewInput::Prepare {
                    path,
                    download_config,
                    ..
                } => (download_config, path),
                NewInput::Install { prepared, .. } => (&prepared.download_config, &prepared.path),
            };
            // Check before constructing any network work; installation checks again after consent.
            target(path)?;
            let config = DownloadConfig::decode(config_bytes)?;
            let downloader = sources::downloader(&config)?;

            match input {
                NewInput::Prepare {
                    path,
                    version,
                    download_config,
                    rcon_requested,
                    loader,
                } => {
                    if loader.is_some() {
                        loader::prepare_installer_plan(&downloader).await?;
                    }
                    let (path, release) = {
                        let (updates, mut receiver) =
                            tokio::sync::watch::channel::<Option<Update>>(None);
                        let checkpoint = sender.clone();
                        let observe = move |update| {
                            checkpoint.checkpoint()?;
                            updates.send_replace(Some(update));
                            Ok(())
                        };
                        drive_vanilla_progress(
                            &sender,
                            &mut receiver,
                            prepare_vanilla(path, &version, rcon_requested, &downloader, &observe),
                        )
                        .await?
                    };
                    let prepared_loader = match loader {
                        Some(request) => Some(
                            loader::prepare_for_new(
                                &downloader,
                                release.id(),
                                &request,
                                &sender,
                                map_loader_event(request.family),
                            )
                            .await?,
                        ),
                        None => None,
                    };
                    let prepared = Prepared {
                        path,
                        release: release.encode()?,
                        download_config,
                        loader: prepared_loader,
                    };
                    sender.send(NewEvent::Prepared(Box::new(prepared))).await?;
                    Ok(())
                }
                NewInput::Install {
                    prepared,
                    accept_eula,
                    no_git,
                    git,
                    rcon,
                } => {
                    let prepared_loader = prepared.loader.clone();
                    let prepared_plan = if prepared_loader.is_some() {
                        Some(loader::prepare_installer_plan(&downloader).await?)
                    } else {
                        None
                    };
                    let (mut installed, server_root) = {
                        let (updates, mut receiver) =
                            tokio::sync::watch::channel::<Option<Update>>(None);
                        let checkpoint = sender.clone();
                        let observe = move |update| {
                            checkpoint.checkpoint()?;
                            updates.send_replace(Some(update));
                            Ok(())
                        };
                        drive_vanilla_progress(
                            &sender,
                            &mut receiver,
                            install_vanilla(
                                prepared,
                                accept_eula,
                                no_git,
                                git,
                                rcon,
                                &downloader,
                                observe,
                            ),
                        )
                        .await?
                    };

                    if let Some(prepared_loader) = prepared_loader {
                        match loader::install_on_existing_server(
                            server_root,
                            prepared_loader.family,
                            Some(prepared_loader.loader_version),
                            false,
                            &downloader,
                            prepared_plan,
                            &sender,
                            map_loader_event(prepared_loader.family),
                        )
                        .await
                        {
                            Ok(modded_server) => {
                                installed.server = modded_server;
                            }
                            Err(error) => {
                                if sender.checkpoint().is_err()
                                    || error
                                        .chain()
                                        .any(|cause| cause.downcast_ref::<InstallCancelled>().is_some())
                                {
                                    return Err(error);
                                }
                                installed.loader_error = Some(PortableError::from(error));
                            }
                        }
                    }

                    let _ = sender
                        .send(NewEvent::Installed(Box::new(installed)))
                        .await;
                    Ok(())
                }
            }
        }))
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum NewInput {
    /// Validate the directory and resolve fixed artifacts without writing any files.
    Prepare {
        path: PathBuf,
        version: String,
        download_config: Vec<u8>,
        #[serde(default)]
        rcon_requested: bool,
        #[serde(default)]
        loader: Option<LoaderRequest>,
    },
    /// Install precisely the artifact(s) resolved before the frontend asked for consent.
    Install {
        prepared: Prepared,
        accept_eula: bool,
        no_git: bool,
        git: bool,
        rcon: Option<RconOptions>,
    },
}

/// Opaque library-owned artifact and the original source configuration, reusable across Hosts.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepared {
    path: PathBuf,
    release: Vec<u8>,
    download_config: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    loader: Option<PreparedLoader>,
}

/// An explicit override or a request to generate a fresh GUID password on the Host.
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RconOptions {
    pub port: Option<u16>,
    pub password: Option<String>,
}

impl std::fmt::Debug for RconOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RconOptions")
            .field("port", &self.port)
            .field("password", &self.password.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum NewEvent {
    Progress {
        message: String,
        bytes: Option<(u64, Option<u64>)>,
    },
    Notice(String),
    Publishing {
        message: String,
    },
    // Keep frequent progress events small; the final result is allocated only once.
    Installed(Box<Installed>),
    Prepared(Box<Prepared>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Installed {
    pub server: ServerInfo,
    pub git: Option<git::Outcome>,
    pub java_major: Option<u32>,
    pub rcon: RconStatus,
    /// Present only when `--loader` was requested, vanilla creation succeeded,
    /// and subsequent loader installation failed (leaving a clean v4 Vanilla server).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader_error: Option<PortableError>,
}

/// Creation result for the requesting frontend; Debug deliberately hides the password.
#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RconStatus {
    Created { port: u16, password: String },
    Disabled,
    UnsupportedVersion,
}

impl std::fmt::Debug for RconStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created { port, .. } => f
                .debug_struct("Created")
                .field("port", port)
                .field("password", &"[redacted]")
                .finish(),
            Self::Disabled => f.write_str("Disabled"),
            Self::UnsupportedVersion => f.write_str("UnsupportedVersion"),
        }
    }
}

fn map_loader_event(family: LoaderFamily) -> impl Fn(AddLoaderEvent) -> Option<NewEvent> + Clone {
    move |event| match event {
        AddLoaderEvent::Stage {
            stage: AddLoaderStage::Publish,
            message,
        } => Some(NewEvent::Publishing { message }),
        AddLoaderEvent::Stage { message, .. } => Some(NewEvent::Progress {
            message,
            bytes: None,
        }),
        AddLoaderEvent::DownloadProgress {
            message,
            received,
            total,
        } => Some(NewEvent::Progress {
            message,
            bytes: Some((received, total)),
        }),
        AddLoaderEvent::InstallerAttempt { source, attempt } => Some(NewEvent::Progress {
            message: format!("Running {family} installer (attempt {attempt} · {source})..."),
            bytes: None,
        }),
        AddLoaderEvent::Notice(notice) => Some(NewEvent::Notice(notice)),
        AddLoaderEvent::Completed(_) => None,
    }
}

async fn drive_vanilla_progress<T, Fut>(
    sender: &StreamSender<NewEvent>,
    receiver: &mut tokio::sync::watch::Receiver<Option<Update>>,
    work: Fut,
) -> Result<T>
where
    Fut: std::future::Future<Output = Result<T>>,
{
    tokio::pin!(work);
    let mut watch_closed = false;
    loop {
        tokio::select! {
            result = &mut work => return result,
            changed = receiver.changed(), if !watch_closed => {
                if changed.is_err() {
                    watch_closed = true;
                    continue;
                }
                let update = receiver.borrow_and_update().clone();
                if let Some((message, bytes)) = update {
                    sender.send(NewEvent::Progress { message, bytes }).await?;
                }
            }
        }
    }
}

async fn prepare_vanilla(
    path: PathBuf,
    version: &str,
    rcon_requested: bool,
    downloader: &Downloader,
    observe: &(impl Fn(Update) -> Result<()> + ?Sized),
) -> Result<(PathBuf, mojang::ServerRelease)> {
    let (path, _) = target(&path)?;
    observe(("Resolving Minecraft version...".into(), None))?;
    let release = mojang::resolve_server(downloader, version, &|event| {
        observe(download_update(event))
    })
    .await?;
    validate_rcon(&release, rcon_requested)?;
    Ok((path, release))
}

async fn install_vanilla(
    prepared: Prepared,
    accept_eula: bool,
    no_git: bool,
    git: bool,
    rcon: Option<RconOptions>,
    downloader: &Downloader,
    observe: impl Fn(Update) -> Result<()> + Send + Sync + 'static,
) -> Result<(Installed, PathBuf)> {
    git::validate_options(git, no_git)?;
    observe(("Checking target directory...".into(), None))?;
    let (path, missing) = target(&prepared.path)?;
    let release = mojang::ServerRelease::decode(&prepared.release)?;
    validate_rcon(
        &release,
        rcon.as_ref()
            .is_some_and(|options| options.port.is_some() || options.password.is_some()),
    )?;
    if missing {
        fs::create_dir(&path).context("failed to create server directory")?;
    }
    let version = release.id().to_owned();
    let java_major = release.java_major();
    release
        .download_to_with_progress(downloader, &path.join(VANILLA_SERVER_JAR), &|event| {
            observe(download_update(event))
        })
        .await
        .with_context(|| {
            format!("server creation failed in {path:?}; inspect the directory before retrying")
        })?;
    observe(("Writing server configuration...".into(), None))?;
    let mut eula = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path.join("eula.txt"))
        .context("server.jar was downloaded, but creating eula.txt failed")?;
    writeln!(eula, "# {}\neula={}", download::eula::EULA_URL, accept_eula)
        .context("server.jar was downloaded, but writing eula.txt failed")?;
    let rcon_status = if release.release_time().timestamp() < RCON_PROPERTIES_SINCE {
        RconStatus::UnsupportedVersion
    } else if let Some(options) = rcon {
        let port = options.port.unwrap_or(properties::DEFAULT_RCON_PORT);
        let password = options
            .password
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        properties::create_rcon(&path.join("server.properties"), port, &password).context(
            "server.jar and eula.txt were written, but creating RCON server.properties failed",
        )?;
        RconStatus::Created { port, password }
    } else {
        RconStatus::Disabled
    };
    let draft = config::vanilla_server()?;
    let final_path = path.clone();
    let initialized = tokio::task::spawn_blocking(move || initialize::initialize_local(
        LocalInitialization {
            path,
            metadata: MinecraftServerMetadata { version: SERVER_METADATA_VERSION, minecraft_version: version, loader: ServerLoader::Vanilla },
            force: false,
            local: Some(&LocalConfiguration {
                configuration: Some(draft),
                replace_conf: false,
                conf_overrides: Default::default(),
                preserve_conf: false,
            }),
        },
        || observe(("Initializing Local workspace...".into(), None)),
        |_| observe(("Initializing Local workspace...".into(), None)),
        |_| Ok(()),
        |path| if no_git { Ok(None) } else { git::ensure_repository(path, git).map(Some) },
    )).await?
        .with_context(|| format!("server.jar and eula.txt were written to {final_path:?}, but workspace initialization failed; inspect completed files before retrying"))?;
    let mut server = ServerInfo::from_server(&initialized.server)?;
    server.start = launch::describe(&final_path)?.map(StartCommand::from);
    Ok((
        Installed {
            server,
            git: initialized.git,
            java_major,
            rcon: rcon_status,
            loader_error: None,
        },
        final_path,
    ))
}

fn validate_rcon(release: &mojang::ServerRelease, explicitly_requested: bool) -> Result<()> {
    ensure!(
        !explicitly_requested || release.release_time().timestamp() >= RCON_PROPERTIES_SINCE,
        "--rcon-port and --rcon-password require a release at or after 1.9-pre4; Minecraft {:?} was released earlier",
        release.id(),
    );
    Ok(())
}

fn target(path: &Path) -> Result<(PathBuf, bool)> {
    let path: PathBuf = std::path::absolute(path)
        .context("failed to resolve server directory")?
        .components()
        .collect();
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            ensure!(
                !metadata.file_type().is_symlink(),
                "server directory must not be a symbolic link: {path:?}"
            );
            ensure!(
                metadata.is_dir(),
                "server target is not a directory: {path:?}"
            );
            ensure!(
                fs::read_dir(&path)?.next().transpose()?.is_none(),
                "server directory is not empty: {path:?}"
            );
            Ok((fs::canonicalize(path)?, false))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .context("server target has no parent directory")?;
            let parent =
                fs::canonicalize(parent).context("server parent directory must already exist")?;
            ensure!(
                parent.is_dir(),
                "server parent is not a directory: {parent:?}"
            );
            Ok((
                parent.join(
                    path.file_name()
                        .context("server target must name a directory")?,
                ),
                true,
            ))
        }
        Err(error) => Err(error).context("failed to inspect server target"),
    }
}

fn download_update(event: download::Event<'_>) -> Update {
    match event {
        download::Event::AttemptStarted { kind, source } => (
            format!(
                "{} · {source}",
                match kind {
                    download::RequestKind::Metadata => "Reading version metadata",
                    download::RequestKind::File => "Downloading server.jar",
                }
            ),
            (kind == download::RequestKind::File).then_some((0, None)),
        ),
        download::Event::Bytes { received, total } => {
            ("Downloading server.jar".into(), Some((received, total)))
        }
        download::Event::Verifying => ("Verifying server.jar...".into(), None),
        download::Event::AttemptFailed { source, .. } => {
            (format!("Download attempt failed · {source}"), None)
        }
        download::Event::Completed { .. } => ("Download complete".into(), None),
    }
}
