# Complex mod output fixture

`complex-mod.jar` is a metadata-only inspection fixture, not a playable mod.
It contains Forge, NeoForge, Fabric, and native Quilt declarations in one archive.
Each loader also identifies a child and grandchild JAR, including a leaf without
dependencies. A second child declares three native Mods under Forge/NeoForge
and one under Fabric/Quilt. A third child is a plain library without Mod
declarations, retained as an empty metadata node. Forge and NeoForge each
declare two mods in the outer archive;
the `omnibus` ID deliberately occurs independently under every loader.

The declarations cover required, optional, recommended, suggested, breaks, and
conflicts; conjunctive and alternative target/version groups; every supported
version constraint variant; reference-local optional/unless conditions, physical
sides, loading order, reasons, escaped control characters, and manifest/property
version substitution. NeoForge type spelling includes uppercase, lowercase,
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
child paths, and preserved library-only nodes. It captures and compares complete
`info` and `deps` output in both raw and redirected modes before previewing
both commands with inherited console streams. Expected paths remain relative
by running in this directory. Inspect snapshot changes against the fixture
declarations before updating expected output.

For an interactive preview of the colors:

```sh
cargo run --manifest-path mcpm/Cargo.toml -- mod info mcpm/tests/fixtures/complex-mod.jar
cargo run --manifest-path mcpm/Cargo.toml -- mod deps mcpm/tests/fixtures/complex-mod.jar
```
