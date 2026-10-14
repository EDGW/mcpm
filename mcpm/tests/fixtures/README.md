# Complex mod output fixture

`complex-mod.jar` is a metadata-only inspection fixture, not a playable mod.
It contains Forge, NeoForge, Fabric, and native Quilt declarations in one archive.
Each loader also identifies a child and grandchild JAR, including a leaf without
dependencies. Forge and NeoForge each declare two mods in the outer archive;
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

The test first captures and compares complete CLI output with the reviewed `.txt`
files, using both global `--raw` and ordinary redirected output. After all
comparisons pass, it runs `info` and `deps` again with inherited console streams
to show the actual terminal output, including colors when the console is a TTY
and `NO_COLOR` is unset. Expected paths
remain relative by running in this directory. Do not automatically refresh the
expected output: inspect changes against the fixture declarations first.

For an interactive preview of the colors:

```sh
cargo run --manifest-path mcpm/Cargo.toml -- mod info mcpm/tests/fixtures/complex-mod.jar
cargo run --manifest-path mcpm/Cargo.toml -- mod deps mcpm/tests/fixtures/complex-mod.jar
```
