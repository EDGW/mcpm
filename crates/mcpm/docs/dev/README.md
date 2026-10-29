# Development principles

## Executable entry points

- `src/bin/` contains five thin entry points: `mcpm`, `mcpm-server`, `mcpmc`, `mcpm-client`, and `mcpm-tools`.
- `program.rs` owns executable names and the immutable server/client/tools command roots. Use these roots for command hints and examples.
- `cli/` composes common options with each family’s existing argument schema. Client entry points omit SSH and remote-log options.
- `app.rs` owns shared process initialization and dispatch. The binary passes its identity explicitly; renaming a file does not change its command family.
- SSH starts `mcpm-tools remote` by default. Remote executable overrides belong to connection settings, independently of local command roots.

## Server configuration and foreground launch

- Backend `local/java/command.rs` owns Java option/operand boundaries and argument-file expansion. Configuration import uses this model without reparsing command strings.
- `local::mc::server::config` owns TOML types, templates, import classification, validation and publication. Frontends neither generate TOML nor interpret fields independently.
- `config/publication.rs` owns configuration locks and recovery for registration and both TOML files. Registration replacement and conf replacement remain separate intentions.
- `launch/plan.rs` asynchronously prepares an immutable plan. Local preflight and argument validation precede managed downloads; `launch/process.rs` owns child lifetime and temporary argument files. Capability probes never run Minecraft or environment-injected agents.
- Frontend `commands/server/launch` owns local routing, CLI adaptation, Tokio execution and stop notifications. Console/Raw inherit game streams without consuming game stdin.
- Launch progress composes `launch::Event`, `authlib_injector::Event` and generic `download::Event` without a download-layer dependency on servers. `display/server/launch.rs` owns stage text, stderr policy and transfer presentation; shared `Progress` owns indicatif, themes and log coordination. Commands only connect callbacks. Displays are dropped before preparation errors reach diagnostics and cleared before spawning Java.
- Explicit `auto` memory/GC tuning remains unsupported. Omitted fields do not infer policy from examples, host state or legacy snapshots.
- `config/overrides.rs` reuses the configuration schema for typed scalar/list changes. Optional authentication tables are populated only in schema probes, without enabling authentication in actual defaults. Effective configuration passes through the existing codec and validation without rewriting TOML.

## Server authlib-injector

- `local::mc::server::authlib_injector/` owns managed JAR policy, validation, staging, publication and its independent preparation lock. It is server state, outside global resource and cache management.
- `config` owns authentication TOML, import classification and argument generation. General import requires original agent files to exist and be readable; manifest identity confirms authlib-injector. Imported paths remain manual.
- `authlib_injector/metadata.rs` is the sole local metadata codec. It consumes typed `download::authlib_injector::Release` fields without decoding upstream JSON again. Local digests reuse the shared download verifier.
- The frontend maps `--auth` / `-A` to existing configuration overrides and adapts `--auth-jar-policy` to the backend policy. A lazy factory constructs the downloader only when needed, reusing the default BMCLAPI/upstream sources and shared fallback rules.
- Managed `auto` repairs only missing, corrupt or mismatched pairs; `local` forbids downloads; `refresh` fetches the latest release again. Unknown metadata formats and OS errors are not repairable corruption. Manual paths access no managed state.
- One exclusive preparation lock fails immediately on contention and is released before returning the plan. Minecraft holds no runtime lease. Dropping preparation cancels downloads and removes staging; the backend installs no signal handlers. JAR and metadata replacement is sequential, so every preparation revalidates their pair rather than assuming atomic publication.

## Server stash

- `mcpm-backend::local::mc::server::stash` owns snapshot operations, indices, HEAD and locks. `commands/server/stash` owns CLI inputs and local routing; `display/stash` owns presentation.
- `.mcpm/stash/index/<UUID>.json` stores version=1, id, name, created_at (UTC) and backup UUID. `.mcpm/stash/HEAD` stores version=1 and stash UUID. Their sole JSON codecs and validation live in stash/index.rs.
- Each frontend leaf owns its Clap arguments, input validation and execution entry, explicitly selecting a shared or exclusive session. The family only routes commands and carries common settings. List and info declare separate column choices while sharing selection validation.
- Backend `stash/create.rs` owns snapshot publication. If publication and cleanup both fail, its dedicated error retains the publication source chain, an independent cleanup chain and the retained backup path. The frontend reports both chains through the existing diagnostics entry.
- Each stash independently owns a manifest at `.mcpm/data/<backup-UUID>`. New stashes use CacheOnly with contents in the server stash blob. File lists and checksums come from the existing backup manifest, never duplicate authoritative index fields. Corrupt stash data is not silently rebuilt as derived cache state.
- The server RuntimeEnvironment stash role lazily selects `.mcpm/stash/blob`. Backup accepts an explicit cache through a shared creation implementation without depending on stash concepts.
- Writers retain an exclusive OS lease; readers and verification retain shared leases on `.mcpm/stash/lock`. The lock file is never removed. Before first initialization, readers capture a fixed empty view instead of reading subsequently published state without a lease.
- Switch fully verifies the target before workspace comparison, including force and no-op switches. HEAD advances only after successful application. There is no cross-file transaction or interruption recovery.
- Generated workspace Git ignore rules do not ignore stash; data and cache follow the generation template.

- Cache management preflights writes before dispatch, including verify --autoremove and export destinations. Stash owns path recognition; the generic cache layer has no server dependency. Server metadata entry presence conservatively reserves the stash cache location. --no-protect-stash bypasses the CLI guard; read-only default aliases open existing caches without initialization.

## Build identity

- `build.rs` is the authority for compile-time build identity; `build_info.rs` exposes version metadata and `display/build_info.rs` renders the startup warning.
- The commit ID and worktree state are captured from Git at build time. Only staged or unstaged changes to tracked files count as `modified`; tracked repository files, the Git index, and references are watched to refresh cached metadata.
- `build.rs` appends six commit characters and an optional `.modified` to unofficial Cargo package versions, using `.unknown` without Git metadata. Packaging verifies that all executable versions agree and writes their actual version to the manifest; a supplied tag must match that version.
- Official builds require all of: the release profile, `MCPM_OFFICIAL_RELEASE=1`, `GITHUB_ACTIONS=true`, repository `EDGW/mcpm`, event `push` or `workflow_dispatch`, a tag reference, and a tag equal to `v<Cargo package version>`.
- The release workflow injects the marker for tag builds. Branch, PR, local, and fork builds default to unofficial. Changes to the input environment variables make Cargo regenerate the identity.
- `official` includes official prereleases; it does not mean stable. This marker is not a source signature.
- All five executables share version details and the unofficial release startup warning. The warning goes only to stderr and is independent of log filtering.

## Backup cache fallback

- The v1 backup manifest is the sole authority for file mappings and checksums. Full and cache-only backups share it without persisted cache paths or availability fields.
- `backup/create.rs` shares cache imports and source verification, with CreateStorage selecting full export or manifest-only publication. Stash explicitly selects CacheOnly.
- `backup/sources.rs` owns source selection, full content verification and data inventory rechecks shared by verify/apply; verify_with_cache does not write or generate a target.
- `backup/apply.rs` shares strict and cache-assisted staging/publication. Strict apply never accesses cache; apply_with_cache invokes a lazy opener only for actually missing manifest files. Corruption, invalid types, unreadable files and unexpected files never trigger fallback.
- Cache owns full content hashing and directory protocols; backup checks manifest sizes, selects sources and publishes targets. Frontend code does not interpret cache layout or manifests.
- Apply opens existing caches without initialization; complete backups and empty manifests do not open cache. All sources and staged results must verify before target replacement.
- display/backup owns English notices, per-file provenance and statistics. Cache-only Raw output keeps existing columns; unfiltered risk/dependency notices go to stderr.
- Stash uses CacheOnly creation and passes an existing server-local stash cache opener to verify_with_cache and apply_with_cache, never a global fallback. Switch fully verifies content under its exclusive lease before workspace checks. Deletion preserves blob contents without GC.

## 1. Commands

Commands are classified by reuse as **single entry point commands** or **shared commands**.

### 1.1. Single entry point commands

A command owned by one command family keeps its implementation and CLI schema in that family.
For example, `mcpm init`, `mcpm-server init`, and `mcpm-tools server init` all reuse
`src/commands/server/init/mod.rs`. Executable roots do not duplicate command definitions.

If the command also supports Remote execution, place its Remote entry point and
definition next to the command entry point.

### 1.2. Shared commands

A shared command has multiple entry points. For example, `mcpm-tools mods list` can be
called directly or through `mcpm mods list`, with some differences in
behavior. The `mods` command family corresponds to both `mcpm-tools mods` and
`mcpm mods`; its shared implementation belongs in
`src/commands/supports/mods/`, with internal paths that reflect the command path.
Place its Remote entry point alongside the shared implementation.

The Remote interface differs from the CLI interface. Remote generally exposes
the underlying operation: both `mcpm-tools mods list` and `mcpm mods list` use
one `mod.list` Remote command, passing different arguments to it.

Therefore, the Remote entry point belongs at the common `supports` entry point,
not at either CLI entry point. Every caller of a shared command must use that
entry point instead of maintaining its own implementation.

## 2. Arguments

Arguments can be complex. Each command usually has a primary argument.

### 2.1. Primary arguments

Query commands should generally accept multiple inputs and one input per line
from `stdin`. For example, `mcpm-tools cache info <KEYS>` should accept multiple
keys from both `<KEYS>` and `stdin`.

#### 2.1.1. Primary argument types

When a key can have several types, as with `mcpm-tools mods info` accepting a mod ID,
filename, or name:

- Use `--input <type>` to interpret `stdin`.
- Use `--<specific-type> <value>` to specify an input type explicitly.

**Decide whether each command supports automatic type inference when designing it.**

- If it does, `type` must include `unspecified` as the default.
- If it does not, nonempty `stdin` must require `--input`, or have an explicit
  default agreed on by the developers. With a default, interpret positional
  `<KEYS>` using that default. Without a default, reject positional `<KEYS>` and
  require `--<specific-type> <value>`.

**Example with automatic type inference:**

```plaintext
> mcpm-tools mods info --help
...
      --file <FILE>          Inspect a file without falling back to modid or name; repeat to select more
      --modid <MODID>        Match an exact modid; repeat to select more
      --name <NAME>          Match an exact mod display name; repeat to select more
      --input <TYPE>         Interpret stdin lines as names, files, modids, or automatic filename/modid/name queries [default: unspecified] [possible values: name, file, modid, unspecified]
...
```

These rules are unnecessary when the primary argument has only one type, such
as a BLAKE3 key in the cache commands.

### 2.2. Other arguments

#### 2.2.1. List arguments

Represent a list with both `--<plural> A,B,C` and repeated singular options,
such as `--<singular> A --<singular> B`.

Some arguments are a **single choice**, not a list, and do not follow this rule.
For example, `--source` in `mcpm-tools mods source search` selects one source. In
`cache refresh <KEYS> ident`, `--source` and `--sources` form a list and
follow this section.

**Standard example:**

```plaintext
  -l, --loader <LOADER>      Select loaders, optionally comma-separated; repeat to select more [possible values: forge, neoforge, fabric]
  -L, --loaders <LOADERS>    Select comma-separated loaders; combined with all --loader options [possible values: forge, neoforge, fabric]
```

The final Loader set combines both options.

**Rules for defaults:**

- **The default is the full set:** Any explicit selection replaces all defaults.
- **The default is empty:** Use only the explicitly selected values.
- **The default is a subset:** The semantics depend on the argument. An AI
  working on such an argument **must discuss its semantics with the developer**.

If the default is nonempty but an empty set is meaningful, add `--no-<plural>`.
It conflicts with both `--<plural>` and `--<singular>`.

If the default is a subset and the developer decides explicit values should
*add to* it, add `--no-default-<plural>`. That flag does not conflict with
`--<plural>` or `--<singular>`.

When the empty set is allowed, `--<plural> ','` is also a valid way to select it.

#### 2.2.2. Loader lists

The full Loader set is `neoforge,fabric,forge`. Loader lists generally default
to the full set and do not allow the empty set.

#### 2.2.3. Source lists

The built-in sources are `modrinth,curseforge`. Source lists generally default
to both, reject the empty set, and replace the defaults on explicit selection.

#### 2.2.4. Column lists

Columns control output fields and may have a default subset. Implement them as
follows:

- Explicit columns **replace**, rather than extend, the defaults.
- An empty selection is invalid.
- Provide a compact form, usually with different argument syntax.

**Standard column example:**

```plaintext
  -C <INMFL>          Select columns by letters: I=index, N=name, M=modid, F=filename, L=loaders

  --columns <COLUMNS> Select comma-separated columns, combined with -C and --column
                      [possible values: index, name, modid, filename, loaders]

  --column <COLUMN>   Select one column; repeat to select multiple columns
                      [possible values: index, name, modid, filename, loaders]
```

`-C` accepts single-letter column names without commas.

## 3. Output

Output has three modes: `Raw`, `Console`, and `TUI`. A command may offer several
modes and must list its supported modes in `--help`.

**Every command must support Console and Raw.**

Use `--raw` for Raw. Commands with a TUI may use `--no-ui` for Console;
`--no-ui --raw` is equivalent to `--raw`.

By default, use the first available mode. Automatically select Raw when
`stdout` is not a TTY. Use `stderr` detection for progress output, and send
progress bars to `stderr` by default. Do not use `stdin` for mode detection.

### 3.1. Raw mode

Raw generally emits TSV for automation, one item per line. JSON may be used
when data is hierarchical or contains tabs or newlines and cannot be expressed
as a simple table. An AI must discuss that choice with the developer first.

Raw omits headers by default. `--headers` enables them only in Raw. Raw should
usually emit only the command's primary data.

For example, the primary data of `mcpm-tools cache list` includes a cache BLAKE3
hash, filename, size, SHA-256, and metadata names. A row could look like:

```plaintext
<blake3>\t<filename>\t<sha256>\tident,meta
```

### 3.2. Console mode

Console uses the Primary and Secondary colors to make output easy to read.
It does not need TSV and may show extra data so users need fewer commands.

For example, `mcpm-tools cache list` can show metadata details even though its
primary data consists of BLAKE3, filename, size, SHA-256, and metadata names:

```plaintext
<BLAKE3>  available
  File: <file>
  Size: xxx bytes
  SHA-256: <sha256>
  2 Metas:
    ident:
    ...
    meta:
    ...
```

Showing each Meta here lets users inspect it without another command.

### 3.3. TUI mode

TODO.

### 3.4. Rendering shared data

Each data type should generally have one output format across Raw and Console.
For example, `ModMeta` and `CachedModMeta` should be rendered consistently
wherever they appear. Put shared rendering code in `src/display/xxx`.

Report errors through `diagnostics::error`; use `Progress::report` when a
progress bar is active. The diagnostics entry point owns logging, remote error
marking, and progress coordination. `src/display/error.rs` renders `error:` or
`remote error:` and each `caused by:` layer. Preserve error context; do not
print errors directly or collapse the chain into one line.

Console renderers use `ConsoleWriter` from `src/display/console`:

- Wrap the output stream with `ConsoleWriter::new` at the Console entry point;
  child renderers receive the existing writer.
- Call `subwriter()` for the next indentation level. The single `INDENT`
  constant defines each level as two spaces; renderers do not handwrite it.
- A child writer borrows its parent and can create further children. The
  parent can resume writing after the borrow ends.
- Use standard `write!`, `writeln!`, or `std::io::Write`. Line state persists
  across writes, and empty lines get no indentation.
- Raw writes directly to the original output stream.

```rust
let mut console = ConsoleWriter::new(output);
writeln!(console, "Metadata:")?;
{
    let mut metadata = console.subwriter();
    writeln!(metadata, "meta:")?;
    let mut details = metadata.subwriter();
    writeln!(details, "Status: available")?;
}
writeln!(console, "Next entry")?;
```

## 4. Common command types

### 4.1. `info`

`info` commands generally support multiple primary inputs from both arguments
and `stdin`, following the rules above. An `info` command with complex output
should also support columns.

### 4.2. `list`

Unlike `info`, a `list` command generally has no primary argument. It should
usually support columns, with Raw output suitable for automation.

A tree-shaped list, such as one containing Mods and Nested Mods, may indicate
collapsed children (for example, `(+x mods)` in `mods list`). Leave children
collapsed by default and offer `--expanded` to show them. Commands without
collapsible data do not need `--expanded`; use `--details` for ordinary details.

**Console list behavior:**

- **Default:** Show only the most important columns, such as cache BLAKE3 or
  ModName + ModId. If few columns are shown and the list is not expanded,
  multiple items may share a line without a header, as in `ls`.
- **`--details`:** Select every column by default, without enabling expansion.
- **`--columns`:** Explicit columns require one item per row and a header.
- Console can use a table or labeled field groups such as `Name: value`.
  Separate field groups with a blank line. If each group contains only one
  field, place its optional header once above the list, not above every item;
  no blank lines are needed between items.
- Above 80 items, ask whether the user wants to display the list using `Y/n`.
- Do not provide `--expanded` when there is nothing collapsible.

Examples:

```plaintext
# Detailed table
Name    Id      File
<name>  <id>    <file>
<name>  <id>    <file>
<name>  <id>    <file>

# Detailed field groups

Name: <name>
Id: <id>
File: <file>

Name: <name>
Id: <id>
File: <file>

Name: <name>
Id: <id>
File: <file>

# Single-column list or field groups, case A (short values)
<name>  <name>  <name>  <name>
<name>  <name>  <name>

# Single-column list or field groups, case B (long values)
Header
<name>
<name>
<name>
<name>
<name>
```
