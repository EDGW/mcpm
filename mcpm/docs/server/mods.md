# Server mod inspection

`server mods info` and `server mods deps` use the registered server's loader and mods directory. Both support Console and Raw output, stdin selections, `--file`, `--modid`, `--name`, and `--all`.

```sh
mcpm --ssh 'ssh user@host' server mods info --server /srv/minecraft --all --parallelism 4
mcpm --ssh 'ssh user@host' server mods deps --server /srv/minecraft --modid create
```

With SSH, `.mcpm/server.json`, the mods directory and selections resolve on the remote host. File inputs are relative to its mods directory; `mods/` prefixes are accepted, parent traversal and absolute file inputs are rejected. Minecraft and loader versions remain server-derived.

`--parallelism` defaults to 10 and bounds full scans and lazy modid/name indexing. Multiple independent file inputs remain sequential. Progress and diagnostics are streamed; output formatting stays local. Remote diagnostics follow the global `--remote-logs` option.

The protocol IDs are `server.mods.info` and `server.mods.deps`. Requests accept `server` (default `.`), `selection` and positive `parallelism` (default 10), using the same selection and inspection event formats as [mod info/deps](../remote.md#mod-info-and-deps-contents).

## List

```sh
mcpm --ssh 'ssh user@host' server mods list --server /srv/minecraft --parallelism 4
mcpm --ssh 'ssh user@host' server mods list --server /srv/minecraft --modid create --expanded
```

List supports TUI, Console (`--no-ui`) and Raw. Empty selections list all files. Filters, columns and layout stay local; the TUI uses the same SSH session for remote file summaries and local source lookups. Nested declaration selections retain their original archive identities.

The `server.mods.list` request accepts `server`, `selection` and `parallelism`. Its events are `scope {loaders}`, `scan {completed,total}`, `error {error}`, ordered `item {index,record}`, and `done {files,input_errors,failed}`. Records reuse the mod-list representation with optional selected declaration keys (`records`, each with `metadata` and `record` indices). The completion's failure count includes unique failed scanned files, even if they are not selected; repeated selected rows do not increase that count.
