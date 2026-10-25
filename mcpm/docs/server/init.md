## `mcpm init`

Register an existing Local Server, or create a local Remote workspace that points to an existing remote Local Server. This command neither installs server software nor starts Minecraft.

```sh
# Recognize and register a server on this machine
mcpm init ./server
mcpm init ./server --start-script run.bat

# Register directly using complete metadata
mcpm init ./server --minecraft-version 1.20.1 --loader forge --loader-version 47.3.12
mcpm init ./server --minecraft-version 1.21.1 --loader vanilla

# Skip Git detection and initialization
mcpm init ./server --minecraft-version 1.21.1 --loader vanilla --no-git

# Create a repository in server even when a parent repository exists
mcpm init ./server --force --minecraft-version 1.21.1 --loader vanilla --git

# Initialize a Local Server on the SSH host; the positional path is remote
mcpm --ssh 'ssh user@host' init /srv/minecraft --minecraft-version 1.21.1 --loader vanilla

# Create a Remote workspace here; the positional path is local, --remote is remote
mcpm --ssh 'ssh user@windows' --ssh-mcpm 'C:\mcpm-tools.exe' init ./workspace --remote 'D:\EDGW-26Summer'

# Subsequent commands use the saved connection automatically
mcpm info ./workspace
mcpm mods list --server ./workspace
```

## Arguments

- `[PATH]`: An existing directory, default `.`. Normally the server directory; with global `--ssh`, the remote server directory; with `--remote`, a local workspace directory.
- `--remote <REMOTE_PATH>`: Register local `PATH` as a Remote workspace pointing to this remote directory. Requires global `--ssh`; global `--ssh-mcpm` is optional. The target must already be a loadable Local Server. Conflicts with `--start-script`, `--minecraft-version`, `--loader`, and `--loader-version`.
- `--start-script <FILE>`: Statically recognize this startup script. The path must be relative to and contained within the actual server directory; absolute paths are rejected. Conflicts with all manual version and loader arguments.
- `--minecraft-version <VERSION>`: Minecraft version; alias `--mc-version`. Complete manual metadata registers directly; otherwise this filters recognized candidates. Whitespace-only values are rejected.
- `--loader <LOADER>`: One of `forge`, `neoforge`, `fabric`, or `vanilla`. Known vanilla servers require explicit `vanilla`; missing loader evidence does not imply vanilla.
- `--loader-version <VERSION>`: Loader version, distinct from Minecraft and FML versions. Required for complete manual modded-server registration; prohibited with `vanilla`. Whitespace-only values are rejected.
- `--force`: Replace an existing registration, including a damaged `server.json`. Normal initialization replaces server metadata; `--remote` replaces the connection saved locally. Does not convert between Local and Remote types or replace a directory at the configuration path.
- `--git`: Require a repository in the target itself, creating one even inside a parent repository. An existing target repository or worktree is reused without deletion or recreation. Conflicts with `--no-git`. Applies to the remote Server for SSH initialization, or the local workspace when using `--remote`.
- `--no-git`: Skip Git repository detection and automatic initialization. `.mcpm/.gitignore` is still generated normally. Conflicts with `--git`. Applies to the remote Server for SSH initialization, or the local workspace when using `--remote`.
- `--no-ui`: Disable candidate selection TUI and use Console mode. Multiple candidates require more specific recognition filters. Combined with global `--raw`, uses Raw mode.
- `--full-command`: Show the complete startup command in the result. Console output normally fits one line with `(+N)` for omitted characters; Raw output always retains the complete command.

SSH connection settings and the remote mcpm executable use the existing [global options](../README.md#global-options), rather than separate `server init` arguments.

## Behavior

### Local behavior

Without explicit global SSH options, a Local or unregistered `PATH` initializes on this machine. Successful initialization produces:

```text
.mcpm/
├── .gitignore   # Ignores /stash, /cache and /registration.lock
├── mcpm.json    # version: 1, type: local
└── server.json  # Local server metadata, version: 2
```

Initializing a Local or Remote workspace creates `.mcpm/.gitignore` containing `# Added by mcpm`, a blank line, and the `/stash`, `/cache`, and `/registration.lock` rules. Existing contents are preserved. If the file already ends with the complete block when blank lines are ignored, nothing is appended; otherwise, the complete block is appended after a newline. Repeated initialization with `--force` therefore does not duplicate the block.

Default Git behavior:

- Before writing registration, use Git to discover whether the target already belongs to a repository. Existing repositories, parent repositories, and worktrees are reused.
- When no repository is found, run `git init` in the target directory. Files are not automatically staged or committed.
- Git must be installed on the host performing initialization. An unavailable Git executable, damaged repository, or failed initialization produces an error. Use `--no-git` to skip Git operations.
- Recognition alone or cancelling candidate selection does not initialize Git.
- `--force` also performs Git setup; `--git` requires the target's own repository and `--no-git` skips setup.
- Console mode shows a white one-line notification before the server information, followed by a blank line, reporting whether the repository was created or initialization was skipped because one already exists, along with the target path. SSH results are displayed locally. Every successful initialization ends with a green `Initialized successfully.` after the server information, including when using `--no-git`. Raw output retains its existing server information fields; `--no-git` produces no Git message. `NO_COLOR` disables colors.

mcpm owns `mcpm.json`, which selects the workspace type. mcpm-backend parses and persists Local `server.json`. Legacy directories without `mcpm.json` are Local. Local metadata version 1 remains readable with a warning; reading never rewrites it, and explicit reinitialization writes version 2.

Complete `--minecraft-version` and `--loader` arguments, plus `--loader-version` unless vanilla, initialize directly without reading scripts. Otherwise recognition uses `run.bat` on Windows or `run.sh` on Unix and filters verified candidates against the supplied fields.

Recognition reads scripts and identity evidence without running scripts, Java, installers, or downloads. Common Forge/NeoForge layouts, Forge shims, Fabric installations, and vanilla JARs with version metadata are supported. Static recognition handles literal assignments, quoting, continuations, directory changes, script references, and Java argument files. Dynamic conditions, loops, pipes, unresolved variables, and recursive references may prevent recognition. Successful recognition does not guarantee launchability; supply complete manual metadata when recognition is unavailable.

A unique candidate registers immediately. Multiple candidates open the TUI: Up/Down selects, Enter confirms, and Esc/Ctrl-C cancels. Selection requires local terminal stdout/stderr without `--raw` or `--no-ui`. Console and Raw require an unambiguous candidate; redirected stdout automatically selects Raw.

Registration saves the selected startup snapshot and prints the same information as [`server info`](info.md). Existing registration is protected by default; failed recognition or cancelled selection preserves it.

- `start.script` stores the script location relative to the Server directory, so moving the whole directory retains that relationship.
- `server.json` does not store a startup working `directory`; the directory supplied when opening the Server is used instead.
- Initialization and loading reject absolute script paths, paths escaping through `..`, and symlinks pointing outside the Server directory. Absolute script paths in existing metadata also fail loading.

### Remote behavior

These commands share the same global SSH options but perform different operations:

```text
mcpm --ssh COMMAND [--ssh-mcpm EXE] init <REMOTE_PATH>
mcpm --ssh COMMAND [--ssh-mcpm EXE] init <PATH> --remote <REMOTE_PATH>
```

| Invocation | Positional path | `--remote` value | Registration destination |
|---|---|---|---|
| Without `--remote` | Remote server directory | None | Remote directory, as Local |
| With `--remote` | Local workspace directory | Existing remote Local Server directory | Local directory, as Remote |

Without `--remote`, scripts, platform defaults, and metadata operations belong to the SSH host. Selection and output remain local. Recognition and complete manual arguments work as they do for local initialization.

With `--remote`, mcpm validates the existing remote Local Server and saves its connection locally, without initializing or modifying the remote server. `.mcpm/mcpm.json` contains `{"version":1,"type":"remote"}`. mcpm owns the Remote `.mcpm/server.json`, for example:

```json
{
  "version": 2,
  "ssh": "ssh user@host",
  "ssh_mcpm": null,
  "path": "/srv/minecraft"
}
```

Omitted or null `ssh_mcpm` uses `mcpm-tools` from the remote PATH. Relative SSH option paths, such as `-i keys/key`, resolve from the local workspace. The remote directory retains its remote-platform interpretation.

Later `server init/info` and supported `server mods list/info/deps/source` commands use the saved connection when no global SSH options are supplied. Thus `server init ./workspace --force ...` reinitializes the remote Local Server. To replace the connection saved locally, use the complete `--ssh ... server init ./workspace --remote ... --force` invocation again. Explicit `--ssh` takes precedence and makes the positional path remote; `--ssh-mcpm` alone remains invalid.

`mcpm-tools remote` only opens Local Servers and rejects Remote workspaces, preventing chained Remote references. `server mods check/dep-graph` do not yet support Remote workspaces. Invalid configuration or connection failure reports an error without falling back to local execution.
