//! Checks launch option structure and assembles configured Java and game arguments.

use super::super::{ArgumentList, LaunchOptions, QuickPlayTarget};
use super::LaunchPlanError;
use std::ffi::{OsStr, OsString};

pub(super) fn configuration(field: &'static str, reason: &'static str) -> LaunchPlanError {
    LaunchPlanError::Configuration { field, reason }
}

pub(super) fn validate(options: &LaunchOptions) -> Result<(), LaunchPlanError> {
    let identity = &options.identity;
    if identity.player_name.is_empty() || identity.player_name.contains('\0') {
        return Err(configuration(
            "identity.player_name",
            "must be nonempty and contain no NUL",
        ));
    }
    let uuid: String = identity.uuid.chars().filter(|c| *c != '-').collect();
    if uuid.len() != 32 || !uuid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(configuration(
            "identity.uuid",
            "expected a 128-bit hexadecimal UUID",
        ));
    }
    if options.java.executable.as_os_str().is_empty() {
        return Err(configuration("java.executable", "must not be empty"));
    }
    if options
        .arguments
        .system_properties
        .keys()
        .any(|key| key.is_empty() || key.contains(['=', '\0']))
    {
        return Err(configuration(
            "arguments.system_properties",
            "keys must be nonempty and contain neither '=' nor NUL",
        ));
    }
    if options.window.width.is_some() != options.window.height.is_some()
        || options.window.width == Some(0)
        || options.window.height == Some(0)
    {
        return Err(configuration(
            "window",
            "width and height must both be positive or both absent",
        ));
    }
    if options.quick_play.is_some() && options.server.is_some() {
        return Err(configuration(
            "server",
            "legacy server and quick play are mutually exclusive",
        ));
    }
    if options.natives.directory.is_none() && !options.natives.extract {
        return Err(configuration(
            "natives",
            "disabling extraction requires an existing directory",
        ));
    }
    if !matches!(
        options.rules.platform.os.as_str(),
        "windows" | "linux" | "osx"
    ) {
        return Err(configuration(
            "rules.platform.os",
            "expected windows, linux, or osx",
        ));
    }
    if options.verification.is_some()
        && (options.directories.libraries.is_some()
            || options.directories.assets.is_some()
            || options.directories.client_jar.is_some())
    {
        return Err(configuration(
            "verification",
            "CheckOptions currently checks standard cache/JAR locations; verify custom locations separately",
        ));
    }
    Ok(())
}

pub(super) fn configure_features(options: &mut LaunchOptions) {
    for key in [
        "is_demo_user",
        "has_custom_resolution",
        "has_quick_plays_support",
        "is_quick_play_singleplayer",
        "is_quick_play_multiplayer",
        "is_quick_play_realms",
    ] {
        options.rules.features.entry(key.into()).or_insert(false);
    }
    if options.window.demo {
        options.rules.features.insert("is_demo_user".into(), true);
    }
    if options.window.width.is_some() {
        options
            .rules
            .features
            .insert("has_custom_resolution".into(), true);
    }
    if let Some(quick) = &options.quick_play {
        options
            .rules
            .features
            .insert("has_quick_plays_support".into(), true);
        let feature = match quick.target {
            QuickPlayTarget::Singleplayer(_) => "is_quick_play_singleplayer",
            QuickPlayTarget::Multiplayer(_) => "is_quick_play_multiplayer",
            QuickPlayTarget::Realms(_) => "is_quick_play_realms",
        };
        options.rules.features.insert(feature.into(), true);
    }
}

pub(super) fn decorate(list: &ArgumentList, core: Vec<OsString>) -> Vec<OsString> {
    list.prepend
        .iter()
        .cloned()
        .chain(core)
        .chain(list.append.iter().cloned())
        .collect()
}

pub(super) fn legacy_jvm(os: &str) -> Vec<OsString> {
    let mut values = vec![
        "-Djava.library.path=${natives_directory}".into(),
        "-cp".into(),
        "${classpath}".into(),
    ];
    if os == "osx" {
        values.insert(0, "-XstartOnFirstThread".into());
    }
    values
}

pub(super) fn append_jvm(values: &mut Vec<OsString>, options: &LaunchOptions) {
    for (flag, size) in [
        ("-Xms", options.memory.minimum_mib),
        ("-Xmx", options.memory.maximum_mib),
    ] {
        if let Some(size) = size {
            values.push(format!("{flag}{size}M").into());
        }
    }
    if let Some(size) = options.memory.stack_kib {
        values.push(format!("-Xss{size}k").into());
    }
    for (key, value) in &options.arguments.system_properties {
        let mut argument = OsString::from(format!("-D{key}="));
        argument.push(value);
        values.push(argument);
    }
}

pub(super) fn append_game(
    values: &mut Vec<OsString>,
    options: &LaunchOptions,
) -> Result<(), LaunchPlanError> {
    let mut ensure = |flag: &str, value: Option<OsString>| {
        if !values.iter().any(|item| item == flag) {
            values.push(flag.into());
            if let Some(value) = value {
                values.push(value);
            }
        }
    };
    if options.window.demo {
        ensure("--demo", None);
    }
    if options.window.fullscreen {
        ensure("--fullscreen", None);
    }
    if let (Some(width), Some(height)) = (options.window.width, options.window.height) {
        ensure("--width", Some(width.to_string().into()));
        ensure("--height", Some(height.to_string().into()));
    }
    if let Some(server) = &options.server {
        if server.address.is_empty() || server.port == Some(0) {
            return Err(configuration(
                "server",
                "address must not be empty and port must be positive",
            ));
        }
        ensure("--server", Some(server.address.clone().into()));
        if let Some(port) = server.port {
            ensure("--port", Some(port.to_string().into()));
        }
    }
    if let Some(quick) = &options.quick_play {
        let (name, value) = quick_target(&quick.target);
        if value.is_empty() {
            return Err(configuration("quick_play", "target must not be empty"));
        }
        // Quick-play syntax must be advertised by this profile or explicitly supplied.
        let flag = format!("--{name}");
        if !values.iter().any(|value| value == OsStr::new(&flag)) {
            return Err(configuration(
                "quick_play",
                "profile does not declare the selected quick-play argument",
            ));
        }
    }
    Ok(())
}

pub(super) fn quick_target(target: &QuickPlayTarget) -> (&'static str, &str) {
    match target {
        QuickPlayTarget::Singleplayer(value) => ("quickPlaySingleplayer", value),
        QuickPlayTarget::Multiplayer(value) => ("quickPlayMultiplayer", value),
        QuickPlayTarget::Realms(value) => ("quickPlayRealms", value),
    }
}
