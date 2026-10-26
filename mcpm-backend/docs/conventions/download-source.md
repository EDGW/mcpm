# Download source conventions

- Download sources only translate URLs. They do not interpret metadata, select versions, or install files; the shared download layer does not require capability registration by component.
- A shared download flow owns source ordering, requests, fallback, and file verification. Callers must not duplicate this logic.
- Each download target (such as authlib-injector, Minecraft, or Forge) has a dedicated, unified API that owns upstream request URLs, protocol path construction, metadata encoding and decoding, metadata validation, and field semantics. Reuse existing parsers.
- Callers use only the API's typed operations and data. They must not hardcode request URLs, construct protocol paths, or parse metadata independently.
- Raw responses, JSON values, and protocol DTOs remain private to the wrapper. Do not expose interfaces that let callers bypass it to retain, read, or interpret raw data.
- When the API lacks required information, extend it rather than adding another parsing entry point or exposing raw data. The wrapper also interprets download URLs embedded in responses before passing them to the shared download flow.
- URL translation applies only to protocol-compatible mirrors. The corresponding download target's wrapper adapts incompatible response formats; the URL translation layer does not.

## API and examples

- `download::DownloadSource` only translates addresses. `sources::MappedSource` supports custom directory-prefix mappings, matching the host, port, and path boundary. Mirror URLs are restored only when the original is unambiguous; shared Maven paths do not imply a particular upstream.
- `DownloadConfig` uses the existing `Codec` for TOML. `sources::downloader` assembles the built-in `bmclapi` and `official` sources. Defaults try BMCLAPI before upstream; an explicit list replaces defaults completely. An empty list disables network downloads; no source is appended implicitly.
- `Downloader` tries deduplicated addresses in order. Network, metadata decoding, and verification failures allow fallback; local file errors stop immediately. Parent directories must exist. Files are published after verification without overwriting existing destinations. Dropping the future cancels the operation.
- `download::authlib_injector::{latest, build}` returns a fixed `Release`. Use `Release::download_to` to download it; fallback never selects another version. Raw responses and protocol types are private.
- `Downloader::download_to_with_progress`, `authlib_injector::latest_with_progress` and `Release::download_to_with_progress` report typed events through synchronous callbacks without text output or UI dependencies. Request events distinguish metadata from files and identify the actual source. Each attempt resets byte counters; totals prefer protocol expectations over HTTP size hints. Size hints never replace digest verification.
- Observers must return promptly. Observer failures stop the operation without remote fallback. Failure-notification errors retain the original request error too. Completion follows successful metadata decoding or verified file publication; notification failure does not roll back published files. Dropping the future still cancels the operation.
- Server launch reuses this API through `local::mc::server::authlib_injector`, which owns local metadata, verification and replacement. The shared download layer has no server dependency. Launch lazily constructs the default source downloader; automatic global download configuration loading and cache integration remain unconnected. Callers must enable Tokio I/O and time drivers.

Run from `mcpm-backend`:

```sh
# Inspect a release; add a destination to download the selected build
cargo run --example download_authlib_injector -- latest
cargo run --example download_authlib_injector -- 56 ./authlib-injector.jar
```

An optional configuration file path may follow the destination. Pass `-` as the destination to inspect metadata without downloading. The configuration file uses this TOML format:

```toml
sources = ["bmclapi", "official"]
connect_timeout_seconds = 10
read_timeout_seconds = 30
```

[简体中文](../../docs-zh/conventions/download-source.md)
