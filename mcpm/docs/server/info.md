# `mcpm server info`

```sh
mcpm server info [PATH] [--full-command]
mcpm server info /path/to/server
```

Defaults to the current directory. A Remote workspace automatically supplies its saved SSH connection unless explicit global SSH options are provided. Local loading requires readable, valid `.mcpm/server.json`;
missing or invalid metadata fails the command. Reports the directory name, path,
Minecraft version, loader and loader version. Absent or unknown
loaders appear as `vanilla or unrecognized`.
New Local metadata uses numeric `version: 2`; version 1 is accepted with a warning and is never rewritten merely by reading it. `minecraft_version` stores the Minecraft version; unsupported format versions fail loading.

Terminal output uses bright cyan headings, bright white names and versions,
plain path/working-directory values, and gray labels and full command. Loader colors follow the shared convention: Forge bright
yellow, NeoForge bright red, Fabric bright cyan. `NO_COLOR` disables colors.
Raw and non-TTY output is one TSV row with six fields: name, path,
Minecraft version, loader, loader version, full command. Loader version is empty
for absent or unknown loaders. Control characters are escaped. Errors go to stderr.

When a Remote workspace supplies the connection, output also includes its saved configuration:

- Remote server path.
- SSH command, showing only the first and last three characters with `***` between them. Short commands reveal fewer characters so the middle always stays hidden.
- Remote mcpm executable, or `mcpm` when no override is saved.

Console output shows the saved Remote configuration first, then a short separator and the server information.
Raw output places these three TSV fields first in that order, followed by the six server fields, for nine fields total; no separator is emitted.
`--full-command` never disables SSH masking. Explicit `--ssh` uses the remote PATH directly and does not load a local Remote registration.

The command and its working directory come from the snapshot saved by
[`server init`](init.md); scripts are not reread. Older/manual configurations
without a saved command display `unavailable`. In non-raw mode the command fits
one terminal line with `(+N)` showing the number of omitted characters.
`--full-command` disables truncation; raw output always includes the full command.

## SSH

```sh
mcpm --ssh 'ssh user@host' server info /path/to/server --full-command
```

The server directory and `.mcpm/server.json` are read on the SSH host. Startup arguments retain their source platform for quoting. Console/Raw formatting stays local; protocol command ID: `server.info`.
