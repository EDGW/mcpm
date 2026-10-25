//! Stash cache mutation preflight, read-only access and filesystem alias recognition.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use mcpm_backend::local::{
    cache::{CacheLinkPreference, hash::Blake3Hash, mods::ModsCacheDir},
    mc::server::{MinecraftServer, MinecraftServerMetadata, stash},
};

struct Fixture {
    root: tempfile::TempDir,
    server: MinecraftServer,
    blob: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let server = MinecraftServer::create(
            root.path(),
            MinecraftServerMetadata {
                version: 2,
                minecraft_version: "1.21.1".into(),
                loader: None,
                start: None,
            },
        )
        .unwrap();
        let blob = server.runtime().cache.stash.selection().path().unwrap();
        Self { root, server, blob }
    }

    fn populate(&self) -> (ModsCacheDir, Blake3Hash) {
        let cache = ModsCacheDir::initialize(&self.blob).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar");
        let id = cache.import(&source, CacheLinkPreference::Copy).unwrap();
        (cache, id)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"));
        command
            .current_dir(self.root.path())
            .args(["--raw", "--log-level", "off", "cache", "mods"])
            .stdin(Stdio::null());
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command()
            .arg("--directory")
            .arg(&self.blob)
            .args(args)
            .output()
            .unwrap()
    }
}

fn protected(output: Output) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--no-protect-stash"),
        "{output:?}"
    );
}

fn success(output: Output) {
    assert!(output.status.success(), "{output:?}");
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let directory = entry.file_type().unwrap().is_dir();
            files.insert(
                path.strip_prefix(root).unwrap().to_owned(),
                (!directory).then(|| fs::read(&path).unwrap()),
            );
            if directory {
                visit(root, &path, files);
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn mutation_preflight_preserves_stash_and_override_allows_explicit_changes() {
    let fixture = Fixture::new();
    protected(fixture.run(&["init"]));
    assert!(!fixture.blob.exists());
    assert!(!fixture.root.path().join(".mcpm/stash").exists());
    success(fixture.run(&["init", "--no-protect-stash"]));
    let (cache, id) = fixture.populate();
    let before = snapshot(&fixture.blob);
    let id_string = id.to_string();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar");
    for args in [
        vec!["init"],
        vec!["import", source.to_str().unwrap()],
        vec!["refresh", &id_string, "meta"],
        vec!["refresh", &id_string, "ident"],
        vec!["verify", "--all", "--autoremove"],
        vec!["remove", "--all"],
        vec!["rebuild-index"],
    ] {
        protected(fixture.run(&args));
        assert_eq!(snapshot(&fixture.blob), before, "{args:?}");
    }
    // The override is inherited even by refresh's nested subcommands.
    success(fixture.run(&["refresh", &id_string, "meta", "--no-protect-stash"]));
    success(fixture.run(&["--no-protect-stash", "rebuild-index"]));
    success(fixture.run(&["remove", &id_string, "--no-protect-stash"]));
    assert!(cache.verified_size(&id).unwrap().is_none());
    success(fixture.run(&["import", source.to_str().unwrap(), "--no-protect-stash"]));
    assert!(cache.verified_size(&id).unwrap().is_some());
    cache.remove(&id).unwrap();
    fs::write(cache.content_path(&id), b"corrupt cache content").unwrap();
    protected(fixture.run(&["verify", "--all", "--autoremove"]));
    assert!(cache.content_path(&id).is_file());
    let output = fixture.run(&["verify", "--all", "--autoremove", "--no-protect-stash"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(!cache.content_path(&id).exists());
}

#[test]
fn reads_preserve_cache_and_exports_check_all_destination_caches_before_writing() {
    let fixture = Fixture::new();
    let (cache, id) = fixture.populate();
    let id_string = id.to_string();
    let before = snapshot(&fixture.blob);
    for args in [
        vec!["list"],
        vec!["info", &id_string],
        vec!["verify", "--all"],
        vec!["verify", &id_string, "--structure-only"],
        vec!["export", &id_string, "copy.jar"],
        vec!["export", &id_string, "link.jar", "--mode", "hard-link"],
    ] {
        success(fixture.run(&args));
        assert_eq!(snapshot(&fixture.blob), before, "{args:?}");
    }
    assert_eq!(
        fs::read(fixture.root.path().join("copy.jar")).unwrap(),
        fs::read(cache.content_path(&id)).unwrap()
    );
    let other = Fixture::new();
    other.populate();
    let destination = other.blob.join("injected.jar");
    protected(fixture.run(&[
        "export",
        &id_string,
        "first.jar",
        destination.to_str().unwrap(),
    ]));
    assert!(!fixture.root.path().join("first.jar").exists());
    assert!(!destination.exists());
    success(fixture.run(&[
        "export",
        &id_string,
        destination.to_str().unwrap(),
        "--no-protect-stash",
    ]));
    assert!(destination.is_file());
}

#[test]
fn recognition_handles_missing_paths_dotdot_and_damaged_server_metadata() {
    let fixture = Fixture::new();
    let server = fixture.server.path().canonicalize().unwrap();
    let mixed_case = fixture.root.path().join(".mcpm/STASH/BLOB");
    protected(
        fixture
            .command()
            .arg("--directory")
            .arg(&mixed_case)
            .arg("init")
            .output()
            .unwrap(),
    );
    assert!(!mixed_case.exists());
    assert_eq!(
        stash::cache_owner(&fixture.blob).unwrap(),
        Some(server.clone())
    );
    assert_eq!(
        stash::cache_owner(&fixture.blob.join("child/new")).unwrap(),
        Some(server.clone())
    );
    assert_eq!(
        stash::cache_owner(&fixture.blob.join("../ordinary")).unwrap(),
        None
    );
    fixture.populate();
    if mixed_case.exists() {
        // An alias on case-insensitive volumes must still be protected after creation.
        protected(
            fixture
                .command()
                .arg("--directory")
                .arg(&mixed_case)
                .args(["remove", "--all"])
                .output()
                .unwrap(),
        );
    } else {
        // Separate existing directories on case-sensitive volumes are distinguishable.
        fs::create_dir_all(&mixed_case).unwrap();
        assert_eq!(stash::cache_owner(&mixed_case).unwrap(), None);
    }
    fs::write(fixture.server.metadata_path(), b"damaged metadata").unwrap();
    protected(fixture.run(&["init"]));
    assert!(fixture.blob.exists());
    fs::remove_file(fixture.server.metadata_path()).unwrap();
    assert_eq!(stash::cache_owner(&fixture.blob).unwrap(), None);
    success(fixture.run(&["init"]));
}

#[cfg(unix)]
#[test]
fn aliases_are_protected_and_default_read_commands_never_initialize_stash_cache() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let home = fixture.root.path().join("home");
    fs::create_dir_all(home.join(".mcpm/cache")).unwrap();
    let global = home.join(".mcpm/cache/mods");
    symlink(&fixture.blob, &global).unwrap();
    protected(
        fixture
            .command()
            .env("HOME", &home)
            .arg("init")
            .output()
            .unwrap(),
    );
    let output = fixture
        .command()
        .env("HOME", &home)
        .arg("list")
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert!(!fixture.blob.exists());
    assert!(!fixture.root.path().join(".mcpm/stash").exists());
    fixture.populate();
    let alias = fixture.root.path().join("alias");
    symlink(&fixture.blob, &alias).unwrap();
    let before = snapshot(&fixture.blob);
    protected(
        fixture
            .command()
            .args(["--directory", "alias/../blob", "remove", "--all"])
            .output()
            .unwrap(),
    );
    protected(
        fixture
            .command()
            .args(["remove", "--all", "--directory", "alias"])
            .output()
            .unwrap(),
    );
    protected(
        fixture
            .command()
            .env("HOME", &home)
            .args(["remove", "--all"])
            .output()
            .unwrap(),
    );
    success(
        fixture
            .command()
            .env("HOME", &home)
            .arg("list")
            .output()
            .unwrap(),
    );
    assert_eq!(snapshot(&fixture.blob), before);

    // The supplied server path remains recognizable when blob points outward.
    let outward = Fixture::new();
    fs::create_dir_all(outward.blob.parent().unwrap()).unwrap();
    symlink(&fixture.blob, &outward.blob).unwrap();
    protected(outward.run(&["remove", "--all"]));
    assert_eq!(snapshot(&fixture.blob), before);

    // A dangling link with a missing component before `..` must not recurse forever.
    let cycle = fixture.root.path().join("cycle");
    symlink("absent/../cycle", &cycle).unwrap();
    assert!(stash::cache_owner(&cycle).is_err());
}
