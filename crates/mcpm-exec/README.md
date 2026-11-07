# mcpm-exec

The execution layer is independent of Minecraft operations and presentation.

- `operation::Operation` defines typed inputs and outputs. `SHALLOW` operations always run on the calling host; other operations follow the selected target.
- `service::Executor` owns one flow's session. Local calls invoke `Operation::run` directly, without serialization. Remote calls connect to a reusable `mcpmd link`.
- `Executor::traffic_stats()` is a local query returning `Option<SessionTrafficStats>` for that flow's client-side Session. Local execution has no Session and returns `None`. It forwards `Session::traffic_stats()` without RPCs or frontend formatting; [messaging](src/docs/messaging.md) defines the independent 100 ms / 1 s send/receive windows and lifetime peaks in B/s.
- `operation::{Connection, Target}` are transport-neutral data types, also re-exported by `service`, so business operations do not depend on service implementation.
- `endpoint::Endpoint` selects a per-user Unix socket or Windows named pipe. Unix listeners hold an exclusive instance lock and validate peer UIDs. Windows listeners use a current-user-only ACL and reject remote clients.
- `messaging::relay` forwards channel bodies in both directions, preserving order and propagating finish, abort, and carrier failure.
- `operation::serve_or_forward` executes registered IDs locally and gives unregistered channels, including their original request, to the forwarding callback. Existing operation framing is unchanged.
- `operation::execute` records INFO at the start of local or peer execution. Remote Executor dispatch is also INFO, with the operation ID and classification. Session/Channel handle creation and destruction produce correlated DEBUG lifecycle events.

## Link startup

`Executor::open_with_idle_timeout(target, seconds)` applies the timeout only when starting a new link. `open(target)` uses 600 seconds. `daemon::executable` is the shared local lookup for links and server shims: prefer a sibling `mcpmd`, then the installer record (`~/.mcpm/mcpmd.path` on Unix, `%LOCALAPPDATA%\mcpm\mcpmd.path` on Windows), then PATH. A missing record warns before PATH fallback; malformed records and stale targets fail.

The link sends newline-delimited `service::control::Ready` events over an inherited readiness pipe: zero or more warnings, followed by one terminal ready/already-running/error response. Clients log warnings without treating them as failure. Startup errors preserve their source chain. Clients validate `link.hello` before issuing operations; `mcpmd.hello` validates the remote daemon. Both compare the exact shared build identity, `service::control::VERSION`, produced by `scripts/build.rs` from the workspace release version.

An existing link retains its original idle timeout. Stop it before changing the effective timeout. Client disconnection aborts its forwarded operations without terminating other clients' sessions.

Rebuilding executables does not replace running link processes. After an incompatible-version error, stop that target's link and retry; local mcpm/mcpmd and the remote mcpmd must have matching build identities. `mcpmd link stop --all` also stops other targets and should only be used intentionally.

