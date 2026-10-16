use super::super::LaunchOptions;
use super::config::configuration;
use super::{LaunchPlanError, NativeArchive};
use crate::local::mc::client::{
    LibrariesDirectory, LibraryError, LibraryFileKind, MinecraftVersionManifest,
};
use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};

pub(super) fn resolve_libraries(
    manifest: &MinecraftVersionManifest,
    libraries: &LibrariesDirectory,
    options: &LaunchOptions,
) -> Result<(Vec<PathBuf>, Vec<NativeArchive>), LaunchPlanError> {
    let mut selected = BTreeMap::new();
    for (index, library) in manifest.libraries().enumerate() {
        let context = |source| LaunchPlanError::Library {
            library: library.name.clone(),
            source,
        };
        if library.clientreq == Some(false)
            || !options
                .rules
                .allows(&library.rules)
                .map_err(LibraryError::from)
                .map_err(context)?
        {
            continue;
        }
        selected.insert(
            library
                .coordinate()
                .map_err(LibraryError::from)
                .map_err(context)?
                .key(),
            (index, library),
        );
    }
    let mut selected: Vec<_> = selected.into_values().collect();
    selected.sort_by_key(|(index, _)| *index);
    let mut classpath = Vec::new();
    let mut natives = Vec::new();
    for (_, library) in selected {
        for file in libraries
            .resolve(library, &options.rules)
            .map_err(|source| LaunchPlanError::Library {
                library: library.name.clone(),
                source,
            })?
        {
            tracing::debug!(library = ?file.name, kind = ?file.kind, path = ?file.path, "Selected launch library");
            match file.kind {
                LibraryFileKind::Artifact => classpath.push(file.path),
                LibraryFileKind::Native => natives.push(NativeArchive {
                    path: file.path,
                    excludes: library
                        .extract
                        .as_ref()
                        .map(|e| e.exclude.clone())
                        .unwrap_or_default(),
                }),
            }
        }
    }
    Ok((classpath, natives))
}

pub(super) fn join_classpath(
    paths: &[PathBuf],
    separator: &str,
) -> Result<OsString, LaunchPlanError> {
    let mut output = OsString::new();
    for (index, path) in paths.iter().enumerate() {
        if path
            .as_os_str()
            .as_encoded_bytes()
            .contains(&separator.as_bytes()[0])
        {
            return Err(configuration(
                "classpath",
                "a classpath entry contains the platform separator",
            ));
        }
        if index > 0 {
            output.push(separator);
        }
        output.push(path);
    }
    Ok(output)
}
