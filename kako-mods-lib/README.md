# kako-mods-lib

Local Minecraft client metadata, resource inspection, and mod identification.

## Client integrity checks

`MinecraftDirectory` exposes read-only `libraries()` and `assets()` managers.
They resolve repository paths, select native archives, and load asset indexes;
they do not create directories or download files.

```rust,no_run
use kako_mods_lib::local::mc::client::{
    CheckOptions, CheckPlatform, CheckRules, MinecraftDirectory,
};

# fn main() -> anyhow::Result<()> {
let minecraft = MinecraftDirectory::new(".minecraft")?;
let version = minecraft.version("1.21.1")?;
let rules = CheckRules::new(CheckPlatform {
    os: "linux".into(),
    arch: "amd64".into(),
    version: "6.8".into(),
    bits: 64,
});
let report = minecraft.check(
    &version,
    CheckOptions::default(),
    &rules,
);
println!("intact: {}", report.is_intact());
for issue in &report.issues {
    println!("{:?}: {}", issue.path, issue.problem);
}
# Ok(())
# }
```

`CheckOptions` has independent `libraries`, `assets`, `asset_index`, and
`version_jar` policies. Each accepts `CheckOptionValue::{Size, Hash, Skip}`.
Defaults are `Hash`, `Size`, `Hash`, and `Hash`, respectively. `Size` checks
metadata without reading file contents; `Hash` first checks any declared size,
then SHA-1. A size mismatch stops that file's check before hashing. `Skip`
omits that category. Set all fields to `Skip` to disable checks.

Platform and feature values come from the caller. `rules.platform` uses launcher
OS names (`windows`, `linux`, `osx`), architecture/version strings, and bitness
for native `${arch}` substitution. OS architecture/version restrictions use
whole-string Rust regex matching. Invalid or unsupported patterns are reported
as rule errors. Feature requirements are compared with `rules.features`; absent
features do not match. The last matching rule determines whether a library is
required. Other platforms' libraries and classifiers are skipped.

- `libraries`: checks applicable primary artifacts and selected natives. Maven
  coordinates provide paths for legacy declarations. Later applicable HMCL
  patches override the same Maven identity. Modern SHA-1/size declarations and
  legacy alternative checksums are supported.
- `assets` and `asset_index`: independently check the index and each unique content-addressed object in
  `assets/objects/`. Legacy `assets` IDs select a local index without a known
  checksum. Index flags `virtual` and `map_to_resources` are parsed; generated
  virtual/resource copies are outside this check's scope.

`CheckReport.files` records each inspected file as `Verified`, `Unverified`, or
`Failed`. `CheckReport.issues` preserves typed failures and their source errors,
including missing files, size/hash mismatches, invalid paths/metadata, broken
versions, and a version belonging to another Minecraft root. Independent
failures are collected without stopping the remaining checks. A failed asset
index is not used to locate objects.

`version_jar` checks the client JAR using `downloads.client`, respecting explicit
`jar` aliases and inherited base versions. A skipped asset index is still read
and parsed when object checks need it, without being counted as a checked file.

`is_intact()` requires at least one enabled policy, no issues, and every selected
file satisfying its policy. Missing size metadata under `Size`, or missing SHA-1
under `Hash`, produces `Unverified`. `has_errors()` and `unverified_count()`
distinguish failures from insufficient metadata. These checks do not establish
Java/mod completeness or launchability. Metadata uses the loaded version snapshot.

The runnable example takes explicit platform values:

```sh
cargo run --manifest-path kako-mods-lib/Cargo.toml --example check_client -- \
  /path/to/.minecraft 1.21.1 linux amd64 6.8 64
```

## Check progress

Use `check_with_progress` to observe synchronous events without coupling the
library to a console or UI. `check` calls the same implementation with a no-op
observer.

```rust,no_run
# use kako_mods_lib::local::mc::client::*;
# fn demo(directory: &MinecraftDirectory, version: &MinecraftVersion, rules: &CheckRules) {
let report = directory.check_with_progress(version, CheckOptions::default(), rules, |progress| {
    if let CheckEvent::FileFinished { status } = progress.event {
        println!("step {}/{}: {:?}, checked {}, pending {:?}, {:?}",
            progress.step_number, progress.total_steps, progress.step,
            progress.completed_files, progress.remaining_files, status);
    }
});
# }
```

`CheckProgress` provides the completed files, known pending files, the next file
in the current step, overall counts, and step counts. Callbacks borrow these
slices; copy any information that must outlive the callback. Completed attempts
include failures and unverified files, not just healthy files.

Events are `Started`, `StepStarted`, `PlanUpdated`, `FileFinished`, `StepFinished`,
and `Finished`. A file emits exactly one `FileFinished` event after its attempt;
reading and hashing emit no intermediate events. Overall totals and remaining
counts stay `None` until the asset index is verified and parsed. If that fails,
undiscovered objects are not reported as checked. All exit paths emit `Finished`.

Default progress has five steps: preparation, client JAR, libraries, asset index,
and asset objects. Disabled categories are omitted from the step count.
