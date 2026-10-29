# mcpmc list

List local Minecraft client profiles and their declared mod loader versions.

```sh
mcpmc list [PATH]
mcpmc list "$HOME/Downloads/.minecraft"
mcpmc --raw list .minecraft > clients.tsv
```

`PATH` is the Minecraft root containing `versions/`. When omitted, the last
ancestor directory named exactly `.minecraft` in the current path is selected;
if there is none, the current directory is used. An inferred root is reported
on stderr with a reminder to use `mcpmc list .` to force the current
directory. Explicit paths bypass inference. `--raw` suppresses the hint. Every version subdirectory is loaded in name order. Missing or invalid
version JSON, missing parents, and inheritance cycles appear as `broken` entries
with their error chain. Other profiles remain visible. No files are modified.

`normal` means the version metadata loaded successfully. Supported loaders are
Fabric, Forge, and NeoForge, recognized using HMCL-compatible library, argument,
and patch rules. Profiles without a recognized supported loader show `vanilla`
or `unrecognized` according to the main-class heuristic. Neither label verifies
that the profile is launchable or that its dependencies are downloaded.

Terminal output uses bright white names, green normal badges, red broken badges,
yellow reasons, and the same loader colors as `mods` commands. `NO_COLOR` disables
colors. Redirected stdout and `--raw` produce five tab-separated columns without
a header: index, directory name, state, loaders, detail. Control characters in
names and details are escaped.

Empty results and lists containing broken profiles exit successfully. Root or
scan errors, output errors, or loader inspection failures exit with status `1`.
Loader inspection failures appear as `failed` entries; other rows are retained.
