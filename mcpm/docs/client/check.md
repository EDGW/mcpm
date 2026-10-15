# mcpm client check

Check declared client client JARs, libraries, native archives, assets, and logging configuration files without changing files.

```sh
mcpm client check [VERSION ...] [--directory PATH] [--all]
mcpm client check 1.18.1 "My Modpack" --directory "$HOME/Downloads/.minecraft"
mcpm client check --directory "$HOME/Downloads/.minecraft"
mcpm client check --all --directory "$HOME/Downloads/.minecraft"
mcpm --raw client check --all --libraries hash --assets skip --asset-index skip --version-jar skip --logging skip > checks.tsv
```

Version IDs are directory names shown by `client list`. Multiple IDs are processed
in argument order, with duplicates removed. `--all` checks all discovered versions,
including broken ones, and conflicts with explicit IDs.

Without IDs or `--all`, a terminal selector opens on stderr. Use Up/Down to select
one version, Enter to check, or Esc/Ctrl-C to cancel. Cancellation exits successfully.
The selector requires terminal stdin, stdout, and stderr and is disabled by `--raw`;
noninteractive callers must provide IDs or `--all`.

`--directory` (`-d`) selects the Minecraft root. When omitted, the last directory
named exactly `.minecraft` in the current path is used, falling back to the current
directory. An inferred-root hint suggests `--directory .` to force the current
directory. Raw mode hides that hint.

Each category accepts `size`, `hash`, or `skip`: `--libraries` defaults to
`hash`, `--assets` to `size`, `--asset-index` to `hash`, `--version-jar` to `hash`, and `--logging` to `hash`. Hash mode checks the declared size first, then SHA-1; a size mismatch
stops that file's check before hashing. Size mode reads file metadata only.
Missing required size/hash metadata produces an unverified result. A skipped
index is still read when asset object checks need it. Client JAR checks resolve
inherited versions and explicit `jar` aliases. Logging checks use the effective
launch manifest (including inheritance and HMCL patches) and inspect the declared
`logging.client` file in `assets/log_configs/`. An absent declaration is skipped;
XML contents are not validated.

The frontend constructs `CheckRules` from the current OS, architecture, and OS
version. Optional launch features are unset. Only applicable libraries/natives
are checked. This does not establish mod/Java completeness or launchability.

Use `--strict` to set all five categories to `hash`, or `--shallow` to set
all five to `size`. These presets conflict with each other and with every
individual policy flag (`--libraries`, `--assets`, `--asset-index`,
`--version-jar`, `--logging`), regardless of argument order.

With terminal stdin, stdout, and stderr, the progress display has four lines:

```text
Step 3/6 Checking libraries (version)
[Main Progress Bar]
23/5000 [libraries/example.jar]
[Step Progress Bar]
```

For multiple versions, an additional top bar shows completed/total versions and
the current version. Reports print above the progress area. Default checks have
six steps: preparation, client JAR, libraries, logging configuration, asset index, and asset objects.
Disabled categories are omitted.

Main progress tracks steps; step progress tracks completed file attempts. The
filename identifies the next file being checked, falling back to the last completed
file at the end of a step. No byte-level progress is displayed or emitted by the
backend. Asset object counts become known only after the index passes verification
and is parsed. A failed index leaves that inventory unknown.

Headings/bars use the cyan theme, counts and filenames use bright white, intact
results use green, failures red, and unverified entries/reasons yellow. `NO_COLOR`
retains the layout without colors. `--raw` and redirected output disable progress
and produce five tab-separated columns: version, state, target, path, detail.
Each version has a `summary` row followed by issue and unverified-file rows.
Control characters are escaped. Diagnostics go to stderr.

Exit status is `0` when all selected checks are intact (or an empty `--all` scan);
`1` for failed checks, insufficient verification metadata, input/output errors, or unavailable
interactive selection; and `2` for invalid arguments. All selected versions are
checked before reporting an incomplete-check error.
