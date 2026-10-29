# `mcpm-tools mods dep-graph`

```text
mcpm-tools mods dep-graph build [OPTIONS] <DIR>
```

Scans `.jar` and `.jar.disabled` files in the supplied directory and recursively identifies declared nested JARs. Filesystem subdirectories are not scanned. Valid archives unrecognized by the selected loaders are ignored. Every loader identification failure, including archive read and metadata parse errors, uses Skip: other loaders and later files are still attempted, successful records are retained, and the graph is exported. Directory scanning and output errors still fail the command.

```sh
mcpm-tools mods dep-graph build mods/
mcpm-tools mods dep-graph build mods/ -o graph.json
mcpm-tools mods dep-graph build mods/ --loader fabric --external fabric:minecraft --external fabric:java --output graph.json
```

Writes formatted JSON to stdout by default, without colors or hints. Use `-o <FILE>` or `--output <FILE>` to write to a file, overwriting existing contents. Diagnostics use stderr. Supports `--loader` and `--loaders`; see [loader selection](README.md#loader-selection).

`--external <LOADER:MODID>` adds a known external component; repeat it to add more. Both parts must be nonempty. Repeated identical identifiers add only one external candidate. An existing target node is reused without changing its incoming relations or other candidates; otherwise a new node is created. External candidates have no outgoing edges or version information. Explicit externals are independent of `--loader`/`--loaders` identification filters and may use a custom loader namespace. No externals are added by default.

The JSON contains `format_version: 1`, `identification_failures`, and a `nodes` array sorted by loader and mod ID. Identification failures record the file path, loader and error cause chain; a nonempty list means identification is incomplete. Failures are also reported to stderr but do not make the build return a failure status. Each node has an `identifier` (`loader` and `id`) and `candidates`. Candidate `kind` is `primary_mod`, `nested_mod`, or `external`; external candidates are added only through explicit `--external LOADER:MODID` options. Unresolved targets remain nodes with empty candidate arrays. Minecraft, Java and loader versions are not inferred.

Mod candidates contain `modinfo` (name, mod ID and version) and `outgoing`.
Each edge stores `target` and the complete `dependency`: `kind`, `id`,
`version_requirement`, `side`, `ordering`, and `reason`. Version requirements
retain `all` and `any_of` conditions for a single target's versions. Edges and
dependencies are unordered; no declaration index or condition path is stored.
Explicit `ordering` remains a loader loading-order requirement and is unrelated
to JSON array position. For example:

```json
{
  "target": { "loader": "forge", "id": "library_b" },
  "dependency": {
    "kind": "required",
    "id": "library_b",
    "version_requirement": { "type": "any" },
    "side": "both",
    "ordering": "after_target",
    "reason": null
  }
}
```

The snapshot format remains version 1 because this format has not been released.

Reverse indices and runtime loader instances are omitted. This is a declaration graph: versions are not evaluated, candidates are not selected for loading, and dependency cycles are not treated as loading-order errors. Disabled files still contribute candidates; the current JSON does not record file enabled state or source paths. Use the `test` subcommand below to read and validate snapshots.

## Validate and restore a snapshot

```sh
mcpm-tools mods dep-graph test graph.json
```

`test <FILE>` calls the library JSON reader with the built-in loader registry. It restores all candidates, complete dependencies, `ModInfo.dependencies`, and reverse relations, then prints node count, candidate counts by kind, edge count, empty node count and identification failure count to stdout. It does not print the graph JSON. It does not open mod archives or evaluate version compatibility. Empty nodes and external candidates require no loader instance; mod candidates must resolve their loader in the registry. Array order does not represent dependency or loading order.

The library API is `storage::read_json(input, &loaders)`, accepting an `std::io::Read` source and a `HashMap<LoaderId, ModLoaderMeta>`. It returns `Result<DependencyGraph, storage::error::ReadGraphError>`. The abstraction does not refer to built-in loader implementations.

The reader distinguishes I/O, JSON syntax/encoding, schema, unsupported format version, and graph validation errors. Graph validation rejects duplicate nodes, unknown mod loaders, mismatched registry IDs, candidate/node ID mismatches, cross-loader edges, inconsistent dependency target IDs, and absent target nodes. Unknown JSON fields and invalid candidate/dependency/version variants are rejected. Cycles, repeated candidates and repeated declarations remain valid. Failed imports emit no statistics; the CLI reports the preserved error cause chain through its standard diagnostics.

## View a mod's dependencies

```sh
mcpm-tools mods dep-graph view -i graph.json fabric:examplemod
mcpm-tools mods dep-graph build mods/ | mcpm-tools mods dep-graph view fabric:examplemod
mcpm-tools --raw mods dep-graph view -i graph.json fabric:examplemod
```

`view [-i FILE] <LOADER:MODID>` reads graph JSON from the specified file, or from stdin when `-i` is omitted or set to `-`. Selectors split at the first colon. Unknown nodes and invalid input are reported through standard error-chain diagnostics.

All dependencies start on a new line in a vertical tree with fixed-width indentation. Node labels show IDs and candidate versions. Edge labels retain the relationship category, version requirement, side, ordering and reason. Relation colors match `deps`; node IDs use loader colors. Redirected output, `--raw` and `NO_COLOR` follow the shared terminal color policy. Branch indentation is independent of label widths, so non-ASCII metadata does not affect tree alignment.

In forward mode, a node expands at its shallowest dependency level. Deeper occurrences are marked `[shared]`, even when the shallow occurrence appears later in the printed tree. Repeated occurrences at the same level also fold after one expansion. References to an ancestor are marked `[cycle]`; this is a traversal marker, not a loading-order error. Multiple candidates are displayed as separate primary/nested/external branches, each with its own declarations. `[no candidate]` means the graph node has no known candidates; `[external]` identifies a supplied external component. Neither label evaluates version compatibility, side applicability or actual candidate activation. Snapshots with skipped identification errors show an incomplete-identification summary.

### Ignore targets and select relation types

```sh
mcpm-tools mods dep-graph view -i graph.json fabric:examplemod --ignores java,fabric:helper
mcpm-tools mods dep-graph view -i graph.json fabric:examplemod --ignore java --ignore :helper
mcpm-tools mods dep-graph view -i graph.json fabric:examplemod --no-default-ignores
mcpm-tools mods dep-graph view -i graph.json fabric:examplemod --type required --types optional,recommended
```

| Option | Behavior |
| --- | --- |
| `--ignore <MOD>` | Add one ignored target; repeat to add more. |
| `--ignores <MODS>` | Add comma-separated targets; repeat or combine with `--ignore`. |
| `--no-default-ignores` | Turn off the default ignore for `minecraft`; explicit ignores still apply. |
| `--type <TYPE>` | Select one relation type; repeat to select more. |
| `--types <TYPES>` | Select comma-separated types; repeat or combine with `--type`. |

Ignore selectors match case-sensitively: `loader:name` matches that exact loader and ID; `:name` and `name` match the ID under any loader. Explicit ignores extend the defaults rather than replacing them. Ignores filter the displayed neighboring node: dependency targets in normal mode, declaring source mods in reverse mode. The explicitly requested root remains visible.

Without type options, only `required`, `breaks`, and `conflicts` are shown. Supplying any `--type` or `--types` replaces that default with the union of explicit selections. Accepted types are `required`, `optional`, `recommended`, `suggested`, `breaks`, and `conflicts`; `break` and `conflict` are singular aliases. To show every relation and remove default ignores:

```sh
mcpm-tools mods dep-graph view -i graph.json fabric:examplemod --no-default-ignores --types required,optional,recommended,suggested,breaks,conflicts
```

Filtering applies to every dependency level and prunes traversal through hidden edges. A node can still appear through another visible path. In forward mode, shallowest-level expansion and `[shared]` markers are computed from the filtered graph, so hidden relations do not determine folding. Reverse mode expands each path without shared-node folding. Filters do not modify the stored graph or identification failure records.

### Reverse dependencies

```sh
mcpm-tools mods dep-graph view -i graph.json fabric:library_b --reverse
mcpm-tools mods dep-graph view -i graph.json fabric:library_b -R --types required,optional
```

`--reverse` / `-R` follows incoming declarations to show which mods directly or indirectly refer to the selected node. A child marked `(required by)` declares a required dependency on its parent; other labels retain their original relation categories, including conflicts and incompatibilities. `parent:` introduces the original version requirement and qualifiers for the parent, not a requirement on the child's own version.

Each reverse child shows the exact source candidate's version and primary/nested identity. Other candidates on that source node are not substituted. Reverse traversal then continues through declarations referring to the source node; it does not traverse the source candidate's outgoing dependencies. Because declarations target nodes, this view does not select which candidate of a multi-candidate target satisfies the relation. Empty target nodes and external components can also have dependents.

Type filters use the original declaration kind; ignores match displayed source mods. Default ignores do not hide an explicitly selected root such as `fabric:minecraft`. Reverse mode does not fold shared nodes: every visible path expands fully. Only a reference to an ancestor on the current path stops with `[cycle]`, preventing infinite output. Colors and incomplete-identification summaries work in both directions.

[Mod commands](README.md) · [简体中文](../../docs-zh/mod/dep-graph.md)
