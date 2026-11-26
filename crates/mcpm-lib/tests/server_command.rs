//! Read-only startup command composition uses current TOML, without preparing Java or authentication.

use std::fs;

use mcpm_lib::local::mc::server::launch;

#[test]
fn command_description_preserves_argument_regions_without_resolving_files_or_java() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".mcpm")).unwrap();
    fs::write(root.path().join("launch.toml"), "[process]\nworkingDirectory='absent-work'\n[jvm]\nextraArguments=['-Dfixed=one']\n[entry]\ntype='jar'\npath='missing-server.jar'\n[application]\nextraArguments=['fixed-application']\n").unwrap();
    fs::write(root.path().join("conf.toml"), "[java]\nexecutable='nonexistent-java'\n[jvm.heap]\nmaximum='3G'\n[jvm]\nextraArguments=['-Dextra=two']\n[server]\nnogui=true\nport=25566\nworld='world with spaces'\n[application]\nextraArguments=['custom-application','']\n").unwrap();
    let launch_before = fs::read(root.path().join("launch.toml")).unwrap();
    let conf_before = fs::read(root.path().join("conf.toml")).unwrap();
    let command = launch::describe(root.path()).unwrap().unwrap();
    assert_eq!(command.executable, "nonexistent-java");
    assert!(command.directory.ends_with("absent-work"));
    assert_eq!(
        command.arguments,
        [
            "-Dfixed=one",
            "-Xmx3G",
            "-Dextra=two",
            "-jar",
            "missing-server.jar",
            "fixed-application",
            "nogui",
            "--port",
            "25566",
            "--world",
            "world with spaces",
            "custom-application",
            ""
        ]
    );
    assert_eq!(
        fs::read(root.path().join("launch.toml")).unwrap(),
        launch_before
    );
    assert_eq!(
        fs::read(root.path().join("conf.toml")).unwrap(),
        conf_before
    );
    assert!(!root.path().join("absent-work").exists());
}

#[test]
fn managed_authentication_command_is_described_without_installation() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".mcpm")).unwrap();
    fs::write(
        root.path().join("launch.toml"),
        "[entry]\ntype='class'\nmainClass='example.Main'\n",
    )
    .unwrap();
    fs::write(
        root.path().join("conf.toml"),
        "[java]\n[authlib-injector]\napiUrl='https://example.invalid/auth'\n",
    )
    .unwrap();
    let command = launch::describe(root.path()).unwrap().unwrap();
    assert_eq!(command.arguments.last().unwrap(), "example.Main");
    assert!(command.arguments.iter().any(
        |arg| arg.starts_with("-javaagent:") && arg.ends_with("=https://example.invalid/auth")
    ));
    assert!(!root.path().join(".mcpm/data").exists());
    assert!(!root.path().join("libraries").exists());
}
