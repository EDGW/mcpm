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
python3 mcpm/tests/fixtures/build_complex_mod.py
```

Run the CLI output test with:

```sh
cargo test --manifest-path mcpm/Cargo.toml --test complex_mod_output -- --nocapture
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
cargo run --manifest-path mcpm/Cargo.toml -- mod info mcpm/tests/fixtures/complex-mod.jar
cargo run --manifest-path mcpm/Cargo.toml -- mod deps mcpm/tests/fixtures/complex-mod.jar
```

## Client profiles

`clients/versions/` contains minimal vanilla, Fabric, Forge, and NeoForge profiles
for the client-list CLI tests. `broken/broken.json` is deliberately invalid JSON;
`missing/` deliberately has no matching version JSON. These verify that broken
profiles remain visible without causing a successful directory listing to fail.
