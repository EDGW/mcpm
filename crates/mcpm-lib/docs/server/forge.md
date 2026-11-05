# Forge and NeoForge identification

Identification lives in `mcpm-lib` and receives a statically parsed `ServerLaunch`. It does not execute scripts, Java, or installers, and does not download dependencies.

- `Ok(Some(identity))`: identification succeeded; returns the Minecraft and loader versions.
- `Ok(None)`: the current detector does not handle this entry or family.
- `Err`: required evidence is missing, conflicting, or unreadable. The caller skips the current launch and keeps other identified candidates.

## Forge / NeoForge dispatch

![Forge and NeoForge entry dispatch, FML family selection, launch declaration checks, and version identification](diagrams/forge-neoforge-dispatch.svg)

Relevant implementation:

- [`forge::server::identify`](../../src/local/modloader/impls/fml/forge/server.rs): Forge entry; dispatches Shim separately.
- [`neoforge::server::identify`](../../src/local/modloader/impls/fml/neoforge/server.rs): NeoForge entry; calls the shared FML detector.
- [`fml::server::identify` / `ensure_server_target`](../../src/local/modloader/impls/fml/server.rs): family selection, version extraction, and exclusion of known client targets.
- [`ServerLaunch::option`](../../src/local/mc/server/launch/mod.rs): supports `--name value` and `--name=value`; repeated identical values are allowed; a missing value, blank value, value starting with `-`, or conflicting repeated value is an error.

## Forge Shim identification

![Forge Shim embedded argument reading, explicit-option precedence, dependency-list fallback, and version merging](diagrams/forge-shim-identify.svg)

- Each option uses the explicit launch argument first and falls back to the embedded `Arguments` default.
- Forge version evidence from the dependency list must agree. A declared version is preserved and need not equal the dependency-list version.

Relevant implementation:

- [`forge::server::identify_shim`](../../src/local/modloader/impls/fml/forge/server.rs): reads Shim metadata and merges version sources.
- [`coordinate` / `agree`](../../src/local/modloader/impls/utils/coordinate.rs): extracts versions from full Maven paths and checks nonempty values and consistency across declarations.

Read, parse, and option-lookup failures in the diagrams return `Err`; errors retain their source context. Identification extracts only identity and version evidence and does not guarantee complete dependencies or a launchable server.
