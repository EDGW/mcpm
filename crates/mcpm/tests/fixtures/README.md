# Complex mod output fixture

`complex-mod.jar` is a metadata-only inspection fixture, not a playable mod.
It contains Forge, NeoForge, and Fabric declarations in one archive.
Each loader also identifies a child and grandchild JAR, including a leaf without
dependencies. A second child declares three native Mods under Forge/NeoForge
and one under Fabric. A third child is a plain library without Mod
declarations, retained as an empty metadata node. Forge and NeoForge each
declare two mods in the outer archive;
the `omnibus` ID deliberately occurs independently under every loader.

The declarations cover required, optional, recommended, suggested, breaks, and
conflicts; conjunctive and alternative version conditions; every supported
version constraint variant; physical sides, loading order, reasons, and
manifest/property version substitution. NeoForge type spelling includes uppercase, lowercase,
mixed case, and an omitted type using its default.

Rebuild the deterministic archive with:

```sh
python3 crates/mcpm/tests/fixtures/build_complex_mod.py
```

Run the CLI output test with:

```sh
cargo test --manifest-path crates/mcpm/Cargo.toml --test complex_mod_output -- --nocapture
```

The test checks the loader-specific archive trees, direct Mod counts, relative
child paths, and preserved library-only nodes. It then captures and compares
complete `info`, `deps`, and expanded `list` output with the reviewed `.txt`
files, using both global `--raw` and ordinary redirected output. List snapshots
cover all loaders and a single selected loader, including hierarchical indices
such as `1-neoforge+2-3` and `1+1+1`, plus `--index-type tree` with aligned
first-item labels, compact two-space nesting, and two spaces of name indentation
per nested JAR. Gray middle-dot guides mark repeated ancestor positions in the
tree indices; plain output retains the same guides without colors.
After all comparisons pass, it runs
`info`, `deps`, and expanded `list` in both index styles with inherited console streams
to show the actual terminal output, including colors when the console is a TTY
and `NO_COLOR` is unset. Expected paths
remain relative by running in this directory. Do not automatically refresh the
expected output: inspect changes against the fixture declarations first.

For an interactive preview of the colors:

```sh
cargo run --manifest-path crates/mcpm/Cargo.toml --bin mcpm-tools -- mods info crates/mcpm/tests/fixtures/complex-mod.jar
cargo run --manifest-path crates/mcpm/Cargo.toml --bin mcpm-tools -- mods deps crates/mcpm/tests/fixtures/complex-mod.jar
```

## Client profiles

`clients/versions/` contains minimal vanilla, Fabric, Forge, and NeoForge profiles
for the client-list CLI tests. `broken/broken.json` is deliberately invalid JSON;
`missing/` deliberately has no matching version JSON. These verify that broken
profiles remain visible without causing a successful directory listing to fail.

## Server identification

`server-init/` contains static scripts and deterministic metadata-only JARs for
`server_init.rs`. Empty `.class` entries are inspection markers, not runnable
bytecode. The fixtures do not establish server authenticity or launchability.

```sh
python3 crates/mcpm/tests/fixtures/build_server_fixtures.py
cargo test --manifest-path crates/mcpm/Cargo.toml --test server_init
```

Success cases cover Forge, NeoForge, early NeoForge with Forge argument names,
Forge shim, both Fabric installer layouts, vanilla and bundled vanilla,
nested BAT scripts, multiple identified entries, and ignored unknown JVM
options. The CLI tests copy each fixture to a temporary directory before writing.

Forge/NeoForge identification combines launch options and referenced Maven paths
without opening dependency JARs. The early NeoForge fixture uses the old Forge
option name and a FancyModLoader path, without an explicit group. The Forge shim
fixture obtains family and versions from its embedded list.
Installation integrity is outside server identification. `fake-vanilla` also
succeeds from its declared entry and version metadata without checking the class
file; `partial-unknown` retains the known candidate alongside the unknown command.

`loader-version-conflict` and `game-version-conflict` are historical examples;
an FML component version or dependency JAR's game metadata is not a distribution
version source. Distribution-coordinate conflicts and legacy Forge constants are
covered in `mcpm-lib/tests/fml_server.rs`.

Use `--log-level debug` to see scripts, argument-file expansion, identity evidence,
and failure reasons; `info` shows identification progress and recognized versions.

Failure fixtures used by the CLI suite:

| Fixture | Reason initialization must fail |
| --- | --- |
| `undefined-variable` / `unquoted-variable` | Unresolved expansion or shell word splitting |
| `delayed-batch` | Delayed variable expansion is unsupported |
| `cycle` / `argument-cycle` | Recursive script or nested Java argument-file reference |
| `unknown-main` | No recognized server entry |
| `missing-jar` / `corrupt-jar` | Missing archive or invalid ZIP |
| `no-java` | Script contains no Java launch |
| `duplicate-version` | Conflicting values of the game version option; identical repetitions are accepted |
| `client-target` | Client entry cannot initialize a production server |
| `fabric-auto-missing-version` | Automatic launcher lacks loader version configuration |

These failures are also tried with `--force` to verify that the old configuration
survives. Multiple candidates are rejected in non-TTY/raw mode. Unknown JVM flags
are retained in the full command and do not themselves cause rejection.

`dynamic-control`, `dynamic-batch`, `dynamic-command`, and `nested-shell` remain
manual inspection examples, not assertions of the old syntax-error behavior.
Unsupported lines/blocks are skipped; command substitution is never evaluated.
Scanner boundary regressions use minimal inline fixtures in `mcpm-lib/tests/server.rs`.

Fabric server identification trusts declared loader/intermediary coordinates without
opening dependency JARs. `fabric-version-conflict` succeeds using the intermediary
version; the selected game JAR is read only if that version evidence is absent.
Installer launchers read embedded install.properties; shaded loader metadata is
read only when no loader coordinate supplies a version.

## Dependency matching problems

[`dependency-problems/mods/`](dependency-problems/README.md) contains metadata-only
mods covering every loader-supported relation category with matched, unmatched,
unknown and missing-candidate cases. It also includes cross-loader identity
mismatches and isolated roots. Its README documents expected states and commands
for previewing `--rule`, `--problem` and colors. Rebuild with:

```sh
python3 crates/mcpm/tests/fixtures/build_dependency_problems.py
```
