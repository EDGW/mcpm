# `mcpm server info`

```sh
mcpm server info [PATH]
mcpm server info /path/to/server
```

Defaults to the current directory. Requires readable, valid `.mcpm/server.json`;
missing or invalid metadata fails the command. Reports the directory name, path,
Minecraft version, loader and loader version. Absent or unknown
loaders appear as `vanilla or unrecognized`.
The metadata requires numeric `version: 1` for its format version and
`minecraft_version` for the Minecraft version; unsupported format versions fail loading.

Terminal output uses bright cyan headings, bright white names and versions, and
gray paths and labels. Loader colors follow the shared convention: Forge bright
yellow, NeoForge bright red, Fabric bright cyan. `NO_COLOR` disables colors.
Raw and non-TTY output is one TSV row with five fields: name, path,
Minecraft version, loader, loader version. The last field is empty for absent or
unknown loaders. Control characters are escaped. Errors go to stderr.
