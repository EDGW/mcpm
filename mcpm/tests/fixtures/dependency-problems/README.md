# Dependency graph problems

`mods/` contains deterministic metadata-only JARs for inspecting match predicates,
rule filtering and tree colors. These are not playable mods. Rebuild with:

```sh
python3 mcpm/tests/fixtures/build_dependency_problems.py
```

Each `case_KIND_LOADER` root declares four dependencies of the named kind:

| Target suffix | Expected result | Evidence |
| --- | --- | --- |
| `_matched` | Matched | Installed `1.0.0`, requirement `*` or `[1,)` |
| `_unmatched` | Unmatched | Installed `1.0.0`, requirement `>=2` or `[2,)` |
| `_unknown_LOADER` | Unknown | Fabric's unsupported `>release` predicate; FML's unsupported non-ASCII candidate version |
| `_missing` | Unmatched | No candidate is installed; displayed as `[no candidate]` |

Unknown examples intentionally produce warning logs. Their metadata can be
identified successfully, but matching cannot be determined by the current
predicate implementation. In particular, FML's non-ASCII example documents a
limitation of our local comparator, not a claim that Maven rejects that version.

Categories follow the declarations each loader can actually express:

| Loader | Categories |
| --- | --- |
| Fabric | required, recommended, suggested, breaks, conflicts |
| Forge | required, optional |
| NeoForge | required, optional, breaks, conflicts |

For breaks/conflicts, a `Matched` target is a problem and an `Unmatched` target
is healthy. `Unknown` is yellow for all categories. Required/optional/recommended/
suggested invert the matched/unmatched colors relative to breaks/conflicts.

`case_cross_loader_forge` and `case_cross_loader_neoforge` depend on `fabric_only`.
That ID is installed under Fabric only, so the FML targets have no candidates.
`isolated` has no dependencies and exercises isolated-root filtering. Unmatched
and matched targets without dependencies provide additional isolated roots.

From the repository root:

```sh
cargo run --manifest-path mcpm/Cargo.toml --bin mcpm-tools -- mod dep-graph build \
  mcpm/tests/fixtures/dependency-problems/mods -o /tmp/mcpm-dependency-problems.json

# All six categories, including types that are disabled by default.
cargo run --manifest-path mcpm/Cargo.toml --bin mcpm-tools -- mod dep-graph view \
  -i /tmp/mcpm-dependency-problems.json \
  --types required,optional,recommended,suggested,breaks,conflicts \
  --ignore-isolated-root

# Default problem rules: required mismatches/unknowns and matched/unknown incompatibilities.
cargo run --manifest-path mcpm/Cargo.toml --bin mcpm-tools -- mod dep-graph view \
  -i /tmp/mcpm-dependency-problems.json --problem

# One category and loader, with ancestor retention and explicit rule selection.
cargo run --manifest-path mcpm/Cargo.toml --bin mcpm-tools -- mod dep-graph view \
  -i /tmp/mcpm-dependency-problems.json neoforge:case_optional_neoforge \
  --type optional --rule optional:unmatched,unknown
```

No graph JSON snapshots are checked in: generate them from these archives.

External components can be supplied with or without a version. For example:

```sh
cargo run --manifest-path mcpm/Cargo.toml --bin mcpm-tools -- mod dep-graph build \
  mcpm/tests/fixtures/dependency-problems/mods \
  --external fabric:required_missing:0.5 \
  --external forge:required_missing \
  -o /tmp/mcpm-dependency-problems.json
```

The Fabric external above fails the `>=1` requirement; the unversioned Forge
external matches its loader/modid without checking versions. A versioned external
uses its own loader's version predicate. Different versions of the same external
remain separate candidates; identical loader/modid/version inputs are deduplicated.

`--problem` also enables `--ignore-isolated-root` and `--no-default-ignores`.
Explicit `--ignore` / `--ignores` still apply.

For a registered server, the same graph commands are available under
`server mods dep-graph`:

```sh
mcpm mods dep-graph build --server /path/to/server -o /tmp/server-graph.json
mcpm mods dep-graph view -i /tmp/server-graph.json --problem
mcpm mods dep-graph test /tmp/server-graph.json
```

The server build scans its `mods/` directory with its registered loader and adds
versioned externals for Minecraft and the loader from `.mcpm/server.json`.
For Fabric these are `fabric:minecraft` and `fabric:fabricloader`; Forge and
NeoForge use `forge:minecraft` / `forge:forge` and
`neoforge:minecraft` / `neoforge:neoforge`, respectively. `--server` defaults to
the current directory. Additional `--external` options remain available;
`view` and `test` operate on saved graphs without requiring a server directory.

Run the default problem view directly with `mcpm mods check --server /path/to/server`.
This builds and inspects the graph in memory. When no trees remain under the
`--problem` filters, it prints `Check passed. No problems found.` in green
(plain text in raw mode).
If mod identification fails, diagnostics are reported and an empty problem view
produces `Check incomplete. Some mod files could not be identified.` instead of
the success message.
