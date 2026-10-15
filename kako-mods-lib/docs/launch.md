# Local client launch API

The launch API lives in `local::mc::client::launch`. It operates on installed
versions and local files. Login, token refresh, downloading resources/Java, and
loader installation are separate responsibilities.

```rust
use kako_mods_lib::local::mc::client::{
    ClientPlatform, MinecraftDirectory, RuleEnvironment,
    launch::{Identity, LaunchOptions},
};

let directory = MinecraftDirectory::new("/games/.minecraft")?;
let version = directory.version("1.21.1")?;
let rules = RuleEnvironment::new(ClientPlatform {
    os: "linux".into(), arch: "amd64".into(), version: "6.8".into(), bits: 64,
});
let mut options = LaunchOptions::new("/path/to/java", Identity::offline("Player"), rules);
options.memory.minimum_mib = Some(512);
options.memory.maximum_mib = Some(4096);
options.directories.game = Some(version.path().canonicalize()?);
options.window.width = Some(1280);
options.window.height = Some(720);

let plan = directory.plan_launch(&version, &options)?;
// Review plan.jvm_arguments(), main_class(), game_arguments(), classpath(), etc.
let prepared = plan.prepare()?;
let mut process = prepared.spawn()?;
let status = process.wait()?;
```

## Three stages

- `MinecraftDirectory::plan_launch(&version, &options)` resolves inheritance,
  patches, applicable libraries, arguments, paths, and resource mapping tasks.
  It reads metadata/indexes but never writes files, invokes Java, or starts a
  process. Broken versions and versions belonging to another root are rejected.
- `LaunchPlan::prepare(self)` validates required local files, optionally verifies
  resources, probes the configured Java when required, creates game/working
  directories, extracts natives, and creates legacy virtual/resources copies.
  No download or game process creation occurs. Rebuild a plan after installing
  missing resources or changing metadata, especially legacy asset indexes.
- `PreparedLaunch::spawn(self)` starts Java directly with `Command::args`, checks
  the target OS against the host, and returns `LaunchProcess`. Arguments are
  individual `OsString` values, never a shell command.

`LaunchPlanError`, `ArgumentError`, `PreparationError`, and `SpawnError` retain
stage-specific typed errors and their sources. Ordinary child-process operations
return `std::io::Result`.

## Configuration

`LaunchOptions::new(java, identity, rules)` requires the Java executable, supplied
identity, and target JVM platform. Every other group has defaults. Public fields
can be configured directly; constructor defaults remain usable with small overrides.

| Group | Fields / behavior | Default |
| --- | --- | --- |
| `java` | `executable`, `version_check` | Required executable; `AtLeastManifest` |
| `identity` | `player_name`, `uuid`, `access_token`, `user_type`, `xuid`, `client_id`, `user_properties` | Required identity |
| `rules` | Platform OS/architecture/version/bitness and feature map | Required platform |
| `directories` | `game`, `working`, `libraries`, `assets`, `client_jar` | Root, game, root/libraries, root/assets, inherited/aliased JAR |
| `arguments` | `main_class`, `jvm`, `game`, `system_properties`, `classpath_prepend`, `classpath_append`, `variables`, `unknown_variables`, `version_name`, `version_type` | Resolved metadata; unknown placeholders error |
| `memory` | `minimum_mib`, `maximum_mib`, `stack_kib` | No implicit memory flags |
| `window` | `width`, `height`, `fullscreen`, `demo` | No override, false, false |
| `launcher` | `name`, `version` | mcpm, library package version |
| `logging` | `Manifest`, `Disabled`, `Custom { file, argument }` | Manifest |
| `natives` | `directory`, `extract`, `maximum_bytes` | `versions/<version>/<version>-natives`, true, 1 GiB total uncompressed limit |
| `process` | `clear_environment`, `environment`, `stdin`, `stdout`, `stderr` | Inherit parent environment and standard streams |
| `quick_play` | `QuickPlay { target, log_path }`; target `Singleplayer`, `Multiplayer`, or `Realms` | None |
| `server` | Legacy `ServerOptions { address, port }` | None |
| `verification` | Optional existing `CheckOptions` | None; required files still must exist |

Relative directory overrides resolve against the Minecraft root. Relative Java
paths containing a separator resolve against the caller's current directory.
A bare executable name such as `java` is resolved by the system through PATH;
there is no Java discovery or automatic installation. `rules.platform` should
represent the selected JVM (including translated/emulated architectures).

`JavaVersionCheck::{AtLeastManifest, ExactManifest, Skip}` determines how
`prepare` uses `java -version`. Without a manifest `javaVersion.majorVersion`,
there is no declared requirement to probe. A minimum-version check does not
establish compatibility with every newer Java release.

For identity, use `Identity::offline(name)` for the Java offline-player UUID and
token `0`, or `Identity::online(name, uuid, token)` for supplied credentials.
Online defaults to user type `msa`; optional account fields remain configurable.
Offline identity does not grant access to authenticated servers. `Secret::new`
sets a token; `Secret::expose` provides explicit access. Identity and plan Debug
output omit credentials/argument values. Raw argument accessors contain secrets
and should only be logged after caller-controlled redaction.

## Argument composition

JVM and game `ArgumentList` each have:

- `replace: None` uses effective manifest arguments. `Some(vec![])` explicitly
  removes that manifest argument list.
- `prepend` inserts arguments before the chosen base list.
- `append` inserts arguments after the base and structured configuration.

The order is prepend, resolved/replacement base, structured configuration, append.
Structured JVM configuration adds memory, system properties, and logging.
Structured game configuration supplies window/demo/fullscreen/legacy-server flags
when not already present. JVM memory/property duplicates retain their argument
order; explicit append arguments can override them according to JVM semantics.

String and rule-controlled object arguments are supported. Rule objects accept a
single string or an array of strings. Each document's patches are normalized before external inheritance, then parent
arguments precede child arguments. HMCL `root: true` profiles are rebuilt from
their authoritative patches; the cached aggregate fields are not appended again.
Inspection metadata remains available separately from `version.launch_manifest()`.
This follows HMCL's [structural resolution](https://github.com/HMCL-dev/HMCL/blob/main/HMCLCore/src/main/java/org/jackhuang/hmcl/game/DefaultGameRepositorySnapshot.java). Libraries select the last
applicable Maven identity and retain declaration order for classpath construction.
Native archives selected via `natives` are extracted rather than added to classpath;
modern classifier artifacts remain classpath entries.

Legacy `minecraftArguments` are tokenized with shlex before substitution; no shell
expansion/execution occurs. Manifests without JVM arguments get native-library
path/classpath defaults, plus `-XstartOnFirstThread` on macOS. Names, paths, and
replacement values containing spaces retain their argument boundaries.

Built-in placeholders include authentication/account fields, version name/type,
launcher name/version, game/assets/natives/library directories, asset index name,
classpath and separator, window resolution, and Quick Play fields. Custom values
use `arguments.variables`; they cannot replace resolved reserved values. Values
are substituted once, without recursively interpreting placeholders inside tokens.
`UnknownVariablePolicy::Preserve` explicitly retains unresolved placeholders.

Width and height must both be positive or both absent. Typed resolution/demo and
Quick Play options enable their corresponding feature rules. Known unset feature
flags default to false; caller-supplied features remain available. Quick Play
requires the selected argument to be declared by the effective profile and cannot
be combined with legacy `server` options. Its default log path is
`game/logs/quickPlay.json`.

## Preparation and process ownership

Manifest logging uses the local `assets/log_configs/<id>` file. Custom logging
arguments support `${path}`. Native exclusions are manifest prefix exclusions.
Archive paths cannot escape the destination, archive symlinks are rejected, and
writing through destination symlink directories is rejected. Files are staged
before replacement. `natives.maximum_bytes` bounds total uncompressed extraction.
Legacy `virtual` and `map_to_resources` indexes produce their required copies.
Preparation may leave created directories, extracted files, or completed mappings on failure.

By default, natives are extracted into `versions/<version>/<version>-natives`
under the Minecraft root. A supplied native directory overrides this path.
Directories and extracted files remain after preparation, process exit, or dropping
an unstarted launch. Later launches extract into the same directory, replacing
matching files. With `natives.extract = false`, an explicit existing directory is
required. Dropping a running process does not kill it.

`ProcessStream::{Inherit, Null, Piped}` controls each standard stream. Environment
entries use `Some(value)` to set and `None` to remove; `clear_environment` clears
inherited variables. `LaunchProcess::child_mut` exposes pipe access and advanced
standard child operations. Use `wait_with_output` to drain piped output while
waiting, or drain pipes concurrently before using `wait`.

Resource verification is opt-in via `verification: Some(CheckOptions { ... })`.
Its report is returned in `PreparationError::Verification` if incomplete. Existing
checks currently target standard cache/JAR locations, so combining this option
with custom libraries/assets/JAR paths is rejected explicitly. Without it, required
JARs, libraries, selected native archives, logging files, indexes, and mapped objects
must exist; full content-addressed asset completeness is not implied.

## Example program

The example only prints the plan unless `--run` is explicitly supplied:

```sh
cargo run --manifest-path kako-mods-lib/Cargo.toml --example launch_client -- \
  /path/to/.minecraft 1.21.1 /path/to/java Player linux amd64 6.8 64
```

Append `--run` to prepare and start Minecraft. It uses offline identity, a 4 GiB
maximum heap, and the version directory as game directory. No CLI launch command
is added by this library interface.

## Structured diagnostics

The library emits `tracing` events and never installs a global subscriber or
chooses console formatting. Callers can install their own subscriber. mcpm uses
`tracing-subscriber` with global `--log-level off|error|warn|info|debug|trace`.

Launch plan/prepare/spawn spans retain the version ID. Info events describe key
stages, per-archive native extraction, PID and exit status. Debug events include
resolved paths/platform/library selections, parsed Java major version, and the
final executable plus redacted argument list. Trace events show individual native
entries/exclusions, declared sizes, legacy mappings and required-file inspections.
Game streams are not captured or converted into tracing events.

`LaunchPlan::redacted_arguments()` provides the diagnostic view without changing
execution arguments. It masks supplied identity credentials and common
credential switches/properties. Custom application-specific credentials should
use environment values rather than raw argument logging. Environment values are
never recorded. Raw `arguments()` remains available for intentional inspection.
