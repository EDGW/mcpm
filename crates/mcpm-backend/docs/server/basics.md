# Basics

Identity operations read or establish what a server is, and publish the launch configuration that
goes with it. Shared execution and failure semantics are in [README](README.md).

| Operation | Classification | Input | Output |
| --- | --- | --- | --- |
| `server.info` | Host | `InfoInput { path }` | `ServerInfo` |
| `server.init.recognize` | Host | `Recognize { path, start_script, minecraft_version, loader, loader_version }` | `StreamingOutput<RecognizeEvent>` |
| `server.init` | Host | `Initialize { path, metadata, force, no_git, git, local }` | `StreamingOutput<InitEvent>` |
| `server.init.prepare_remote` | Host | `PrepareRemoteInput { path, no_git, git }` | `Initialized` |

`Recognize` filter fields may be null; `Initialize.local` may be null. `PrepareRemoteInput.git`
and `no_git` are booleans.

## `server.info`

Reads server identity, the historical startup snapshot, and whether `launch.toml` and `conf.toml`
exist. Configuration files are checked only for presence; their contents are not parsed and no
launch plan is generated. Missing or invalid registration returns an error, since there is no
identity to report.

## `server.init.recognize`

Reads the startup script and identity evidence statically, without executing scripts, Java, or
installers. A damaged existing registration does not prevent recognition; recognition itself
writes no files, creates no Git repository, and publishes no configuration.

Version and Loader options filter recognized candidates; they do not fabricate missing fields.
Each candidate contains metadata, source line, main class, and a configuration draft.

The stream emits stage events and finally the candidate list. The frontend selects a candidate
locally and submits the selected draft to `server.init`; the Host retains no recognition session.

## `server.init`

Publishes one identity supplied manually or obtained through recognition. It owns validation, Git
preparation, configuration publication, and the workspace marker.

`local` selects local publication: `Some` publishes identity, launch configuration, and
`conf.toml` through mcpm-lib; `None` registers identity only. Manual identity has no configuration
draft, so it does not generate `launch.toml`.

Git behavior:

| Input | Behavior |
| --- | --- |
| Default | Reuse the target repository, a parent repository, or a worktree; create a repository only when none exists |
| `git` | Require a repository in the server directory itself |
| `no_git` | Skip discovery and creation |

`git` and `no_git` cannot both be true. Files are not staged or committed automatically. A damaged
repository or failed Git command returns an error.

The stream emits validation and writing stages and finally `Initialized { server, git,
publication }`. mcpm-lib coordinates publication of identity, launch, and `conf.toml`; the Local
workspace marker is written after publication succeeds.

## `server.init.prepare_remote`

Prepares an existing remote server before a Remote workspace points at it. It validates that the
target is a Local Server and performs the requested Git discovery or creation; it publishes no
server configuration and returns `publication: None`.

Returns `Initialized { server, git, publication: None }`. The local stub is written afterward by
Shallow `workspace.register_remote`; this operation does not modify the local stub or create a
local Git repository.
