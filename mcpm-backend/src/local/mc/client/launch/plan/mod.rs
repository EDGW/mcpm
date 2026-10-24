//! Read-only client launch planning and resolved launch inputs.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
};

use super::{ArgumentError, LaunchOptions, LoggingOptions, arguments};
use crate::local::mc::client::{LibrariesDirectory, MinecraftDirectory, MinecraftVersion};

mod config;
mod error;
mod libraries;

pub use error::LaunchPlanError;

use config::*;
use libraries::*;

/// A selected native archive and the manifest's extraction exclusions.
#[derive(Debug, Clone)]
pub struct NativeArchive {
    pub path: PathBuf,
    pub excludes: Vec<String>,
}

/// Immutable resolved launch inputs. Arguments may contain credentials: Debug
/// intentionally shows only counts/paths; arguments() is an explicit raw accessor.
#[derive(Clone)]
pub struct LaunchPlan {
    pub(super) options: LaunchOptions,
    pub(super) directory: MinecraftDirectory,
    pub(super) version: MinecraftVersion,
    pub(super) executable: PathBuf,
    pub(super) game: PathBuf,
    pub(super) working: PathBuf,
    pub(super) natives: PathBuf,
    pub(super) classpath: Vec<PathBuf>,
    pub(super) archives: Vec<NativeArchive>,
    pub(super) required: Vec<PathBuf>,
    pub(super) jvm: Vec<OsString>,
    pub(super) main_class: String,
    pub(super) game_args: Vec<OsString>,
    pub(super) java_major: Option<u32>,
    pub(super) mappings: Vec<(PathBuf, PathBuf)>,
}

impl LaunchPlan {
    pub fn executable(&self) -> &Path {
        &self.executable
    }
    pub fn game_directory(&self) -> &Path {
        &self.game
    }
    pub fn working_directory(&self) -> &Path {
        &self.working
    }
    pub fn natives_directory(&self) -> &Path {
        &self.natives
    }
    pub fn classpath(&self) -> &[PathBuf] {
        &self.classpath
    }
    pub fn native_archives(&self) -> &[NativeArchive] {
        &self.archives
    }
    pub fn jvm_arguments(&self) -> &[OsString] {
        &self.jvm
    }
    pub fn main_class(&self) -> &str {
        &self.main_class
    }
    pub fn game_arguments(&self) -> &[OsString] {
        &self.game_args
    }
    pub fn required_files(&self) -> &[PathBuf] {
        &self.required
    }
    pub fn arguments(&self) -> Vec<OsString> {
        self.jvm
            .iter()
            .cloned()
            .chain(std::iter::once(self.main_class.clone().into()))
            .chain(self.game_args.iter().cloned())
            .collect()
    }
    pub fn options(&self) -> &LaunchOptions {
        &self.options
    }
}

impl fmt::Debug for LaunchPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaunchPlan")
            .field("executable", &self.executable)
            .field("main_class", &self.main_class)
            .field("game", &self.game)
            .field("classpath_entries", &self.classpath.len())
            .field("jvm_arguments", &self.jvm.len())
            .field("game_arguments", &self.game_args.len())
            .finish_non_exhaustive()
    }
}

impl MinecraftDirectory {
    /// Resolves a read-only plan. Relative directory overrides are rooted here;
    /// relative Java paths containing a separator are rooted at the caller's pwd.
    /// Missing runtime files are checked by prepare(), not by argument resolution.
    #[tracing::instrument(name = "launch.plan", skip_all, fields(version = ?version.name()), err)]
    pub fn plan_launch(
        &self,
        version: &MinecraftVersion,
        options: &LaunchOptions,
    ) -> Result<LaunchPlan, LaunchPlanError> {
        tracing::info!(root = ?self.path(), "Building launch plan");
        validate(options)?;
        let canonicalize = |path: &Path| {
            path.canonicalize().map_err(|source| LaunchPlanError::Io {
                operation: "canonicalize launch root",
                path: path.to_owned(),
                source,
            })
        };
        let absolute = |path: &Path| {
            std::path::absolute(path).map_err(|source| LaunchPlanError::Io {
                operation: "resolve absolute launch path",
                path: path.to_owned(),
                source,
            })
        };
        let root = canonicalize(self.path())?;
        if canonicalize(version.root_path())? != root {
            return Err(LaunchPlanError::RootMismatch {
                version: version.name().into(),
            });
        }
        let manifest = version
            .launch_manifest()
            .map_err(|source| LaunchPlanError::Version { source })?;
        let resolve = |path: &Path| {
            if path.is_absolute() {
                path.to_owned()
            } else {
                root.join(path)
            }
        };
        let mut options = options.clone();
        configure_features(&mut options);
        tracing::debug!(platform = ?options.rules.platform, java_version_check = ?options.java.version_check, "Using launch platform rules");
        let game = options
            .directories
            .game
            .as_deref()
            .map(&resolve)
            .unwrap_or(root.clone());
        let working = options
            .directories
            .working
            .as_deref()
            .map(&resolve)
            .unwrap_or(game.clone());
        let libraries = LibrariesDirectory::new(
            options
                .directories
                .libraries
                .as_deref()
                .map(&resolve)
                .unwrap_or(root.join("libraries")),
        );
        let assets = options
            .directories
            .assets
            .as_deref()
            .map(&resolve)
            .unwrap_or(root.join("assets"));
        let natives = options
            .natives
            .directory
            .as_deref()
            .map(&resolve)
            .unwrap_or_else(|| {
                root.join("versions")
                    .join(version.name())
                    .join(format!("{}-natives", version.name()))
            });
        let executable = if options.java.executable.is_absolute()
            || options.java.executable.components().count() == 1
        {
            options.java.executable.clone()
        } else {
            absolute(&options.java.executable)?
        };
        let main_class = options
            .arguments
            .main_class
            .clone()
            .or_else(|| manifest.main_class().map(str::to_owned))
            .ok_or(LaunchPlanError::MissingMetadata { field: "mainClass" })?;
        if main_class.is_empty()
            || main_class.starts_with('-')
            || main_class.chars().any(char::is_whitespace)
        {
            return Err(configuration(
                "main_class",
                "expected a nonempty Java class name",
            ));
        }
        let (mut classpath, archives) = resolve_libraries(manifest, &libraries, &options)?;
        let jar = if let Some(path) = &options.directories.client_jar {
            resolve(path)
        } else {
            absolute(
                &version
                    .launch_jar_path()
                    .map_err(|source| LaunchPlanError::Version { source })?,
            )?
        };
        classpath.push(jar);
        classpath.splice(
            0..0,
            options
                .arguments
                .classpath_prepend
                .iter()
                .map(|path| resolve(path)),
        );
        classpath.extend(
            options
                .arguments
                .classpath_append
                .iter()
                .map(|path| resolve(path)),
        );
        let mut seen = BTreeSet::new();
        classpath.retain(|path| seen.insert(path.clone()));
        let mut required = classpath.clone();
        if options.natives.extract {
            required.extend(archives.iter().map(|item| item.path.clone()));
        }
        let separator = if options.rules.platform.os == "windows" {
            ";"
        } else {
            ":"
        };
        let classpath_value = join_classpath(&classpath, separator)?;
        let mut variables = BTreeMap::new();
        let mut mappings = Vec::new();
        let mut game_assets = assets.clone();
        let index_id = manifest
            .asset_index()
            .map(|info| info.id.as_str())
            .or(manifest.assets());
        if let Some(id) = index_id {
            let manager = crate::local::mc::client::AssetsDirectory::new(&assets);
            let path = manager.index_path(id)?;
            required.push(path.clone());
            // Mapping requires the index inventory. Normal modern planning does
            // not require the index to exist until preparation.
            if path.is_file() {
                let index = manager.read_index(id)?;
                if index.virtual_assets {
                    game_assets = assets.join("virtual").join(id);
                }
                for (name, object) in index.objects {
                    if index.virtual_assets || index.map_to_resources {
                        let source = manager.object_path(&object.hash)?;
                        if index.virtual_assets {
                            mappings.push((
                                source.clone(),
                                super::super::path::within(&game_assets, &name)?,
                            ));
                        }
                        if index.map_to_resources {
                            mappings.push((
                                source,
                                super::super::path::within(&game.join("resources"), &name)?,
                            ));
                        }
                    }
                }
            }
        }
        let identity = &options.identity;
        let uuid: String = identity.uuid.chars().filter(|c| *c != '-').collect();
        for (key, value) in [
            ("auth_player_name", identity.player_name.clone()),
            ("auth_uuid", uuid),
            ("auth_access_token", identity.access_token.expose().into()),
            (
                "auth_session",
                format!(
                    "token:{}:{}",
                    identity.access_token.expose(),
                    identity.uuid.replace('-', "")
                ),
            ),
            ("user_type", identity.user_type.clone()),
            ("user_properties", identity.user_properties.to_string()),
            ("auth_xuid", identity.xuid.clone().unwrap_or_default()),
            ("clientid", identity.client_id.clone().unwrap_or_default()),
            (
                "version_name",
                options
                    .arguments
                    .version_name
                    .clone()
                    .unwrap_or_else(|| version.name().into()),
            ),
            (
                "version_type",
                options
                    .arguments
                    .version_type
                    .clone()
                    .unwrap_or_else(|| manifest.version_type().unwrap_or("release").into()),
            ),
            ("launcher_name", options.launcher.name.clone()),
            ("launcher_version", options.launcher.version.clone()),
            ("assets_index_name", index_id.unwrap_or_default().into()),
            ("classpath_separator", separator.into()),
        ] {
            variables.insert(key.into(), value.into());
        }
        for (key, path) in [
            ("game_directory", game.as_path()),
            ("assets_root", assets.as_path()),
            ("game_assets", game_assets.as_path()),
            ("natives_directory", natives.as_path()),
            ("library_directory", libraries.path()),
        ] {
            variables.insert(key.into(), path.as_os_str().to_owned());
        }
        variables.insert("classpath".into(), classpath_value);
        if let (Some(width), Some(height)) = (options.window.width, options.window.height) {
            variables.insert("resolution_width".into(), width.to_string().into());
            variables.insert("resolution_height".into(), height.to_string().into());
        }
        if let Some(quick) = &options.quick_play {
            variables.insert(
                "quickPlayPath".into(),
                quick
                    .log_path
                    .as_deref()
                    .map(&resolve)
                    .unwrap_or(game.join("logs/quickPlay.json"))
                    .into_os_string(),
            );
            let (key, target) = quick_target(&quick.target);
            variables.insert(key.into(), target.into());
        }
        for (key, value) in &options.arguments.variables {
            if variables.contains_key(key) {
                return Err(ArgumentError::ReservedVariable { name: key.clone() }.into());
            }
            variables.insert(key.clone(), value.clone());
        }
        let mut jvm = if let Some(values) = &options.arguments.jvm.replace {
            values.clone()
        } else {
            let raw = manifest.jvm_arguments().iter().collect::<Vec<_>>();
            if raw.is_empty() {
                legacy_jvm(&options.rules.platform.os)
            } else {
                arguments::evaluate(raw, &options.rules, "JVM")?
            }
        };
        append_jvm(&mut jvm, &options);
        let logging = match &options.logging {
            LoggingOptions::Disabled => None,
            LoggingOptions::Custom { file, argument } => Some((resolve(file), argument.clone())),
            LoggingOptions::Manifest => manifest
                .client_logging()
                .map(|logging| {
                    crate::local::mc::client::AssetsDirectory::new(&assets)
                        .log_config_path(&logging.file.id)
                        .map(|path| (path, logging.argument.clone()))
                })
                .transpose()?,
        };
        if let Some((path, argument)) = logging {
            if variables.contains_key("logging_configuration_path") {
                return Err(ArgumentError::ReservedVariable {
                    name: "logging_configuration_path".into(),
                }
                .into());
            }
            variables.insert(
                "logging_configuration_path".into(),
                path.clone().into_os_string(),
            );
            jvm.push(
                argument
                    .replace("${path}", "${logging_configuration_path}")
                    .into(),
            );
            required.push(path);
        }
        jvm = decorate(&options.arguments.jvm, jvm);
        let mut game_args = if let Some(values) = &options.arguments.game.replace {
            values.clone()
        } else {
            let raw = manifest.game_arguments().iter().collect::<Vec<_>>();
            if raw.is_empty() {
                arguments::legacy(manifest.legacy_launch_arguments().ok_or(
                    LaunchPlanError::MissingMetadata {
                        field: "arguments.game or minecraftArguments",
                    },
                )?)?
            } else {
                arguments::evaluate(raw, &options.rules, "game")?
            }
        };
        append_game(&mut game_args, &options)?;
        game_args = decorate(&options.arguments.game, game_args);
        let jvm =
            arguments::expand_all("JVM", jvm, &variables, options.arguments.unknown_variables)?;
        let game_args = arguments::expand_all(
            "game",
            game_args,
            &variables,
            options.arguments.unknown_variables,
        )?;
        required.extend(mappings.iter().map(|(source, _)| source.clone()));
        let mut seen = BTreeSet::new();
        required.retain(|path| seen.insert(path.clone()));
        tracing::debug!(main_class = ?main_class, classpath_entries = classpath.len(), native_archives = archives.len(), required_files = required.len(), "Launch plan ready");
        tracing::debug!(java = ?executable, game_directory = ?game, working_directory = ?working, natives_directory = ?natives, "Resolved launch paths");
        Ok(LaunchPlan {
            directory: self.clone(),
            version: version.clone(),
            java_major: manifest.java_version().map(|j| j.major_version),
            options,
            executable,
            game,
            working,
            natives,
            classpath,
            archives,
            required,
            jvm,
            main_class,
            game_args,
            mappings,
        })
    }
}
