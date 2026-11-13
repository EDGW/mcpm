# Basics

Registration and inspection commands. Target selection is defined in [README](README.md).

## `mcpm info`

```text
mcpm info [OPTIONS] [PATH]
```

### Arguments

| Argument | Description |
| --- | --- |
| `[PATH]` | Server directory. Defaults to `.`, the current directory. Without `--ssh`, a registered Remote workspace uses its saved remote server instead. |

### Options

| Option | Description |
| --- | --- |
| `--full-command` | Show the complete startup command generated from `launch.toml` and `conf.toml`. It does not unmask SSH. |

### Output

Console shows the name, path, Minecraft version, loader and version, and whether `launch.toml` and `conf.toml` exist on the Host that owns the server. A Remote workspace also shows the remote path, masked SSH, and mcpmd path.

Legacy versions 1–3 cannot be used by launch or other server operations. Console `info` first reports an upgrade error with `mcpm info --upgrade` guidance, then displays the legacy registration and returns a nonzero exit status. Raw `info`, including redirected stdout, fails without printing metadata. Legacy `--full-command` does not prepare or generate a launch command. Reading preserves the original file.

The error ends at `requires an upgrade`; a separate shared `Tip:` explains that the metadata is deprecated and gives the upgrade command. A completed upgrade uses success styling and is followed by a blank line before server information.

Raw is TSV without a header, in this field order:

```text
name path minecraft-version loader loader-version start-command
```

A Remote workspace prepends remote path, masked SSH, and mcpmd path. A command not requested or missing configuration is `unavailable`. Missing or invalid registration returns an error.

`--full-command` shares launch argument generation, without starting Java, downloading, or checking executable/JAR existence. V1/V2 historical `start` is ignored. Existing invalid configuration returns an error.

### Examples

```sh
mcpm info ./server
mcpm info ./workspace
mcpm --ssh 'ssh user@host' info /srv/minecraft
mcpm info ./server --full-command
```

## `mcpm info --upgrade`

Upgrade Local registration versions 1–3 to version 4. Ordinary `info` never rewrites registration.

```sh
mcpm info ./server --upgrade
mcpm --ssh 'ssh user@host' info /srv/minecraft --upgrade
```

### Registration formats

- Version 4 requires a non-null `loader`: `{"type":"vanilla"}`, `{"type":"unrecognized"}`, or a concrete loader such as `{"type":"forge","version":"47.4.0"}`.
- Versions 1–3 can be decoded using the old V3 structure only for legacy inspection and explicit upgrade; ordinary server operations require V4. Legacy `loader: null` means Unrecognized until explicitly confirmed. Legacy `start`, including any V1/V2 value, is ignored.
- Other versions fail without rewriting. Remote workspace connection registrations use a separate format and remain unchanged.

### Vanilla confirmation

For a legacy Unrecognized registration, inspect only `launch.toml`. JAR entries read their manifest relative to `process.workingDirectory`; class/module entries use an explicit `mainClass`. No Java execution, downloads, `conf.toml`, or historical `start` are involved.

```text
Set loader to Vanilla? [y(es)/n(o)/C(ancel)]
```

| Answer | Result |
| --- | --- |
| `y`, `yes` | Write V4 Vanilla |
| `n`, `no` | Write V4 Unrecognized |
| `c`, `cancel`, Enter, Esc, Ctrl-C, EOF | Cancel without changing registration |

Answers are case-insensitive. A missing launch file, an implicit module main class, or another main class keeps Unrecognized. Invalid configuration or unreadable JAR evidence fails without committing. Existing concrete loader IDs and versions are preserved. Version 4 is a no-op.

Raw and redirected output reject legacy metadata without writing, including `info --upgrade`. Run the explicit upgrade in Console mode. For SSH and Remote workspaces, files are inspected and written on the server host; confirmation stays local. Changed registration or launch evidence invalidates the pending decision.

Only `.mcpm/server.json` changes. Minecraft version, configuration, worlds, mods, Git, and connection registration are preserved. Vanilla and Unrecognized are displayed separately; their loader-version field is blank. `--full-command` remains available and uses current configuration after the upgrade.

## `mcpm init`

```text
mcpm init [OPTIONS] [PATH]
```

### Arguments

| Argument | Description |
| --- | --- |
| `[PATH]` | Existing directory. Defaults to `.`. Usually the server directory; with `--remote` it is the local Remote workspace; with explicit `--ssh` and no `--remote` it is the remote server directory. |

### Options

| Option | Description |
| --- | --- |
| `--remote <REMOTE_PATH>` | Register PATH as a Remote workspace pointing to an existing remote Local Server. Requires `--ssh`. Conflicts with `--start-script`, manual identity options, and `--foverride-conf`. |
| `--start-script <FILE>` | Statically recognize a startup script inside the server directory, using a relative path. Conflicts with manual identity options. |
| `--start-line <[SCRIPT:]LINE>` | Select a recognized command by its one-based source line. Requires `--start-script`; conflicts with manual identity and `--remote`. SCRIPT is server-root-relative and needed only for nested scripts. |
| `--minecraft-version <VERSION>` | Minecraft version. Alias: `--mc-version`. |
| `--loader <LOADER>` | Loader: `fabric`, `forge`, `neoforge`, or `vanilla`. |
| `--loader-version <VERSION>` | Loader version, distinct from Minecraft and FML versions. |
| `--force` | Replace existing registration and `launch.toml` during server initialization; replace the existing connection registration with `--remote`. Existing `conf.toml` is preserved by default. |
| `--foverride-conf` | Regenerate `conf.toml`, replacing settings and comments. Requires `--force`; conflicts with `--remote`. |
| `--no-git` | Skip Git discovery and creation on the Host that owns the server. Conflicts with `--git`. |
| `--git` | Require a Git repository in the actual server directory itself. Conflicts with `--no-git`. |
| `--no-ui` | Disable the candidate-selection TUI and use Console. |
| `--full-command` | Show the complete startup command generated from `launch.toml` and `conf.toml`. |

### Paths and execution target

| Situation | PATH | Actual write target |
| --- | --- | --- |
| No `--ssh`; Local or unregistered directory | Local server directory | Local server |
| No `--ssh`; registered Remote workspace | Local workspace directory | Remote server in registration |
| `--ssh` without `--remote` | Remote server directory | Specified remote server |
| `--ssh` with `--remote REMOTE_PATH` | Local workspace directory | Local connection registration; REMOTE_PATH is an existing remote Local Server |

The actual target must be a Local Server; a Remote workspace cannot point to another Remote workspace.

### Initialization modes

#### Recognize a startup script

```sh
mcpm init ./server
mcpm init ./server --start-script scripts/start.sh --no-ui
mcpm init ./server --start-script run.bat --start-line 12
mcpm init ./server --start-script run.sh --start-line scripts/start.sh:3
```

Without `--start-script`, the Host's default script is `run.bat` on Windows and `run.sh` elsewhere. The script path must stay inside the server directory.

Recognition statically reads scripts and identity evidence without executing them. Lines with unsupported operators or command substitutions are skipped, as are complete unsupported control-flow blocks. Potential directory/variable changes in skipped text invalidate only the affected state; later commands that depend on unknown state cannot be imported. Insufficient evidence may still require complete manual identity.

Candidates include the configuration draft needed for initialization. Git preparation and configuration writes occur only after a candidate is selected; recognition alone or cancelling selection does not write.

#### Complete manual identity

```sh
mcpm init ./server --minecraft-version 1.21.1 --loader vanilla
mcpm init ./server --minecraft-version 1.20.1 --loader forge --loader-version 47.3.12
```

| Combination | Requirement |
| --- | --- |
| Any non-vanilla Loader | Provide both `--minecraft-version` and `--loader-version` |
| vanilla | Provide explicit `--loader vanilla`; `--loader-version` is prohibited |

Complete manual identity does not read a startup script and does not generate `launch.toml`. Use recognizable startup-script initialization when launch configuration is needed.

#### Partial identity: filter candidates

```sh
mcpm init ./server --loader forge --no-ui
mcpm init ./server --minecraft-version 1.20.1 --no-ui
```

Incomplete identity still runs recognition; supplied fields only filter recognized candidates. No matching candidate fails; multiple matching candidates follow the selection mode below.

#### Candidate selection

| Candidate count | Behavior |
| --- | --- |
| 1 | Used directly without opening the TUI |
| Multiple, terminal stdout/stderr without `--no-ui` | Local TUI cards: Up/Down moves, Enter confirms, Esc or Ctrl-C cancels |
| Multiple, terminal stdout with `--no-ui` or redirected stderr | Console numbered list; enter a number to confirm. Enter without a number, Esc, Ctrl-C, or EOF cancels |
| Multiple, `--raw` or redirected stdout | Error with a structured Tip containing each candidate and a copyable initialization command |

Selection stays local for remote initialization. Console reads the controlling terminal, never redirected stdin; stdin does not select the output mode. Cards and lists show the source, main class, entry, and application-argument summary from the existing configuration draft.

Use `--start-script` plus `--start-line` to select a recognized command without a prompt while retaining its launch configuration. A bare line selects within the root script, not a nested script; unsupported/skipped commands cannot be selected. Retry commands preserve the original PATH, SSH target, and write options, and replace partial identity filters with exact source selection. Commands use POSIX shell quoting on Unix and PowerShell quoting on Windows.

### Files written

| File | Content |
| --- | --- |
| `.mcpm/server.json` | Version-4 server identity registration |
| `.mcpm/mcpm.json` | Local workspace type |
| `.mcpm/.gitignore` | Metadata ignore rules |
| `launch.toml` | Launch configuration imported from the candidate draft; not generated for manual identity |
| `conf.toml` | User settings |

Existing `conf.toml` is preserved by default. Publication fails when a new launch configuration conflicts with the preserved `conf.toml`.

### Reinitialization and replacement

```sh
# Replace registration and launch configuration; preserve existing conf.toml
mcpm init ./server --start-script run.sh --force

# Also regenerate conf.toml
mcpm init ./server --start-script run.sh --force --foverride-conf
```

| Situation | Behavior |
| --- | --- |
| Existing registration or `launch.toml`, no `--force` | Failure |
| `--force` with startup-script initialization | Replace registration and `launch.toml`; preserve `conf.toml` |
| `--force` with manual identity | Replace registration; do not generate `launch.toml`, and remove an existing `launch.toml` |
| `--force --foverride-conf` | Also replace `conf.toml` |
| Existing Local/Remote type | `--force` cannot convert the type |
| `.mcpm/server.json` is a directory | Replacement is rejected |

Publication, type-marker, or connection failure can occur after some files are written. Inspect the actual files before retrying.

### Git

| Option | Behavior |
| --- | --- |
| Default | Reuse the target repository, a parent repository, or a worktree; create a repository in the server directory only when none exists |
| `--git` | Require a repository in the server directory itself; reuse an existing repository |
| `--no-git` | Skip discovery and creation |

Files are not staged or committed automatically. Unless `--no-git` is used, Git must be installed on the Host that owns the server; a damaged repository or failed Git command returns an error. `--force` does not skip Git.

### Remote initialization

```sh
mcpm --ssh 'ssh user@host' init /srv/minecraft \
  --minecraft-version 1.21.1 --loader vanilla --no-git
```

PATH is the remote server directory. Recognition, Git, server registration, and configuration publication all run remotely.

### Remote workspace

```sh
# Create a local entry for an existing remote server
mcpm --ssh 'ssh user@host' init ./workspace --remote /srv/minecraft --git

# Replace the saved local connection and path
mcpm --ssh 'ssh user@new-host' init ./workspace --remote /srv/new-server --force --no-git

# Follow the local entry and reinitialize the remote server
mcpm init ./workspace --force --minecraft-version 1.21.1 --loader vanilla --no-git
```

| Invocation | Effect |
| --- | --- |
| `init ./workspace --remote REMOTE_PATH` | Validate the remote Local Server, perform Git preparation, then write the local connection registration |
| No `--remote`; PATH is a Remote workspace | Initialize the remote server in the registration |
| Existing Remote registration without `--force` | Connection replacement is rejected |
| Local directory is already a Local workspace | Conversion to Remote is not allowed |

Creating a Remote workspace runs Git against the remote server directory; the local workspace does not create a Git repository.

The local workspace stores only the SSH command, optional mcpmd path, and remote directory. The remote server must already be registered as a Local Server; initialize it first otherwise.
