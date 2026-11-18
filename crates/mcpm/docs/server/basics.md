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
| `--full-command` | Generate the complete startup command from `launch.toml` and `conf.toml`. Invalid configuration returns an error. The executable and JAR are not checked for existence. |

### Output

#### Console output

- **Server**: Name, path, Minecraft version, loader type, and loader version.
- **Configuration**: Whether `launch.toml` and `conf.toml` exist.
- **Remote connection**: A Remote workspace also shows the remote path, masked SSH command, and mcpmd path.

#### RCON status

RCON status comes from the server's `server.properties`:

- **Not configured**: Missing files or settings show as `disabled`.
- **Invalid configuration**: Invalid RCON field values and failures to parse `server.properties` are reported separately.
- **Password masking**: Only the first character is shown. A single-character password is fully masked.

#### Raw output

Raw output is TSV without a header, in this field order:

```text
name path minecraft-version loader loader-version start-command
```

A Remote workspace prepends the remote path, masked SSH command, and mcpmd path. The command field is `unavailable` if `--full-command` was omitted or the launch configuration is missing.

#### Registration errors

Missing or invalid registration returns an error. For legacy registration:

- **Console**: Prompts you to run `mcpm info --upgrade`, displays the legacy registration, then exits with a nonzero status.
- **Raw**: Returns an error without printing the registration.

`--full-command` cannot generate a startup command from a legacy registration.

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

- **Version 4**: Requires a non-null `loader`, such as `{"type":"vanilla"}`, `{"type":"unrecognized"}`, or `{"type":"forge","version":"47.4.0"}`. A version 4 registration does not need an upgrade.
- **Versions 1–3**: Read using the legacy V3 format for inspection and explicit upgrade. `loader: null` means Unrecognized, and the legacy `start` field is ignored. Other server operations require version 4.
- **Other versions**: Return an error without rewriting the registration.

Remote workspace connections use a separate format and are unaffected.

### Vanilla confirmation

An existing concrete loader keeps its ID and version. For an Unrecognized registration, `launch.toml` provides the main class:

| Entry type | Main class source |
| --- | --- |
| JAR | Resolve the JAR relative to `process.workingDirectory` and read its manifest. |
| Class / Module | Use the explicit `mainClass`. |

The loader stays Unrecognized if `launch.toml` is missing, a module has no explicit main class, or the main class does not match. Invalid configuration or an unreadable JAR returns an error.

When a vanilla entry is detected, the command asks for confirmation. Answers are case-insensitive:

```text
Set loader to Vanilla? [y(es)/n(o)/C(ancel)]
```

| Answer | Result |
| --- | --- |
| `y`, `yes` | Write V4 Vanilla |
| `n`, `no` | Write V4 Unrecognized |
| `c`, `cancel`, Enter, Esc, Ctrl-C, EOF | Cancel without changing registration |

### Execution requirements

- **Output mode**: Run an explicit upgrade in Console mode. Raw mode rejects the upgrade without writing.
- **Remote targets**: The server host inspects and writes the files; confirmation stays local.
- **Confirmation validity**: A change to the registration or launch evidence invalidates a pending choice.

### Upgrade result

Only `.mcpm/server.json` changes. Vanilla and Unrecognized are displayed separately, both with a blank loader-version field. After the upgrade, `--full-command` uses the current configuration.

## `mcpm init`

Register an existing server or create a Remote workspace that points to one.

In script recognition and manual initialization, `[PATH]` defaults to `.` and selects the server as follows:

| Invocation | Server location |
| --- | --- |
| Explicit `--ssh` | `[PATH]` on the SSH host. |
| No `--ssh`, and `[PATH]` is a Remote workspace | The remote server in the saved registration. |
| No `--ssh`, and `[PATH]` is Local or unregistered | `[PATH]` on this machine. |

Recognition, Git preparation, and writes run on the server host. Candidate selection stays local.

### 1. Recognize a startup script

```sh
mcpm init ./server
mcpm init ./server --start-script scripts/start.sh --no-ui
mcpm init -FIg ./server
```

| Argument or option | Behavior |
| --- | --- |
| `[PATH]` | Existing server or workspace directory. |
| `--start-script <FILE>` | Scan the given server-relative script, including supported references to other scripts. If omitted, try `run.bat` then `start.bat` on Windows, or `run.sh` then `start.sh` elsewhere. Stop at the first script that yields a candidate. |
| `-I`, `--ignore-unknown-vars` | Warn about statements that cannot be interpreted and skip them, keeping the known working directory and variables. |
| `--no-ui` | Use the numbered Console list instead of TUI cards. |
| `--raw` | Disable interaction. |
| `-f`, `--force` | Required if the server is already registered or `launch.toml` exists. Replaces the registration and `launch.toml` while preserving existing `conf.toml` by default. |
| `-F` | Equivalent to `--force --foverride-conf`: replace the registration and launch configuration and regenerate `conf.toml`. |
| `--foverride-conf` | Regenerate `conf.toml`, replacing user settings and comments. Use with `--force`. |
| `-g`, `--git` / `-G`, `--no-git` | Require a repository in the server directory itself, or skip Git entirely. Mutually exclusive. |
| `--full-command` | Show the complete startup command derived from the resulting configuration. |
| `--ssh <COMMAND>` / `--ssh-mcpmd <PATH>` | Select the server host explicitly; `[PATH]` is then on that host. The mcpmd path is optional and requires `--ssh`. |

#### Startup command scanning

- **Script references**: Start at the entry script and follow calls that can be resolved statically, such as `sh scripts/launch.sh` or `call scripts\launch.bat`. Each candidate identifies the script and line containing the Java command.
- **Unsupported syntax**: Skip lines with unsupported operators or command substitutions, as well as unsupported control-flow blocks.
- **Candidate eligibility**: A Java command must establish a server identity and yield a launch configuration draft. Failure to identify or import one command does not discard other candidates.

#### Working directory and variables after skipped statements

- **Default**: Mark the working directory or variables that a skipped statement may change as unknown. If this prevents recognition, the error identifies the source and advises when to retry with `-I`.
- **With `-I`**: Warnings show the skipped statement's script, line, and reason. Previously known state is kept. Control-flow blocks are still skipped as a whole, so Java commands inside them do not become candidates.

Use `-I` only if the skipped statements are irrelevant to the intended launch. For example, after skipping a dynamic `cd`, `server.jar` is resolved against the previously known directory.

#### Server identity and version

- **One matching loader**: Create one candidate with the Minecraft version, loader type, and loader version.
- **Multiple matching loaders**: Create a separate candidate for each match. Choose which server identity to register.
- **No matching loader**: Try vanilla version recognition only for a `java -jar` command whose JAR declares a supported vanilla server main class.

Vanilla Minecraft version sources, in order:

1. **`version.json`**: Read the version ID from the launch JAR.
2. **`META-INF/versions.list`**: Locate the bundled server JAR from the list and verify its `version.json`.
3. **Offline SHA-1 table**: If both metadata files are absent, hash the entire JAR and match it against unmodified official server downloads before `18w47b`.

Invalid or inconsistent metadata rejects the command rather than falling back to SHA-1. Use `--log-level info` to see which source identified the vanilla version.

#### Candidate selection

A single candidate is used directly. With multiple candidates, behavior depends on the [output mode](../README.md#output-modes):

| Mode | Selection |
| --- | --- |
| TUI | Move with Up or Down and confirm with Enter. Esc or Ctrl-C cancels. |
| Console | Enter a candidate number. An empty answer, Esc, Ctrl-C, or EOF cancels. |
| Raw | List the candidates and return an error. Use TUI or Console to choose, or supply complete manual identity. |

Console reads from the controlling terminal, not redirected stdin. Candidates show the script location, main class, entry, and application arguments.

#### Configuration import

Git preparation and writes start after a candidate is selected. The import writes:

| File | Contents |
| --- | --- |
| `.mcpm/server.json` | Version 4 server identity. |
| `.mcpm/mcpm.json` | Local workspace type. |
| `.mcpm/.gitignore` | Metadata ignore rules. |
| `launch.toml` | Entry, fixed arguments, working directory, and explicit environment overrides. |
| `conf.toml` | Recognized Java, memory, GC, and server options. |

Java argument files are expanded during import. Changes to the scripts require reimport.

#### Java agent recognition and import

Resolve each `-javaagent` path relative to the original Java working directory. A missing or unreadable file, or a path to a directory, prevents importing that command.

For `-javaagent:<JAR>=<API_URL>`, the manifest identifies authlib-injector when `Premain-Class` is `moe.yushi.authlibinjector.Premain`. The filename does not matter. Other agents retain their original `-javaagent` argument and options.

When exactly one authlib-injector is recognized:

- **JAR and API address**: Store them as `jar` and `apiUrl` in `[authlib-injector]` in `conf.toml`. The path still points to the existing JAR.
- **Supported JVM properties**: Import each `-Dauthlibinjector.*` property that occurs exactly once and can be represented as a configuration field into `[authlib-injector]`.
- **Other JVM properties**: Keep repeated, unknown, or unrepresentable properties as their original JVM arguments.

A command with more than one recognized authlib-injector is excluded from the candidate list.

#### Configuration publication and recovery

A conflict with the preserved `conf.toml` prevents publication. If publication is interrupted, rerun `init` to recover before [launching the server](launch.md).

### 2. Initialize with complete manual identity

```sh
mcpm init ./server --minecraft-version 1.21.1 --loader vanilla
mcpm init ./server --minecraft-version 1.20.1 --loader forge --loader-version 47.3.12
mcpm --ssh 'ssh user@host' init /srv/minecraft --minecraft-version 1.21.1 --loader vanilla --no-git
```

| Argument or option | Behavior |
| --- | --- |
| `[PATH]` | Existing server or workspace directory. |
| `--minecraft-version <VERSION>` | Required. Alias: `--mc-version`. |
| `--loader <LOADER>` | Required: `vanilla`, `fabric`, `forge`, or `neoforge`. |
| `--loader-version <VERSION>` | Required for a modded server; not allowed with `--loader vanilla`. This is the loader version, not the Minecraft or FML version. |
| `-f`, `--force` | Replace the server registration and remove any existing `launch.toml`. Existing `conf.toml` is preserved by default. |
| `-F` | Equivalent to `--force --foverride-conf`: replace the registration, remove an existing `launch.toml`, and regenerate `conf.toml`. |
| `--foverride-conf` | Regenerate `conf.toml`, replacing user settings and comments. Use with `--force`. |
| `-g`, `--git` / `-G`, `--no-git` | Require a repository in the server directory itself, or skip Git entirely. Mutually exclusive. |
| `--full-command` | Show the complete startup command if available. |
| `--ssh <COMMAND>` / `--ssh-mcpmd <PATH>` | Select the server host explicitly; `[PATH]` is then on that host. The mcpmd path is optional and requires `--ssh`. |

Partial identity returns an error.

Manual initialization writes the Local registration and settings without generating `launch.toml`. Use script recognition to import launch configuration.

### 3. Register a Remote workspace

```sh
mcpm --ssh 'ssh user@host' init ./workspace --remote /srv/minecraft --git
mcpm --ssh 'ssh user@new-host' init ./workspace --remote /srv/new-server --force --no-git
```

| Argument or option | Behavior |
| --- | --- |
| `[PATH]` | Existing local workspace directory. Defaults to `.`. |
| `--remote <REMOTE_PATH>` | Required. Existing, already registered Local Server on the SSH host. |
| `--ssh <COMMAND>` / `--ssh-mcpmd <PATH>` | Explicit `--ssh` is required. The remote mcpmd path is optional. |
| `-f`, `--force` | Replace the saved connection and server path in an existing Remote workspace; cannot convert a Local workspace to Remote. |
| `-g`, `--git` / `-G`, `--no-git` | Require a repository in the remote server directory itself, or skip Git entirely. Mutually exclusive. |
| `--full-command` | Show the remote server's complete startup command if available. |

Registration order:

1. On the SSH host, verify that the target is a registered Local Server and prepare Git.
2. Write the connection registration on this machine: the SSH command, optional mcpmd path, and remote directory.

To initialize the server through a saved connection, use script recognition or manual initialization without `--remote`.

### Default Git behavior

Git reuses a repository in the server directory, a parent repository, or a worktree. If none exists, it creates a repository in the server directory. `--force` does not skip Git. Files are not staged or committed automatically.

### Write limits

- **Replacement**: `--force` cannot change the workspace type or replace a directory at `.mcpm/server.json`.
- **Partial completion**: An interrupted publication or connection can leave files already written. Inspect the target before retrying.

See [common options](../README.md#common-options) for global output and logging flags.
