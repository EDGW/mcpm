# Errors

`mcpm_exec::error` owns the diagnostic model shared by business producers, operation transport,
and presentation. It does not depend on any business crate.

The model exists because `anyhow::Error` carries one linear `source()` chain, while a failed
operation can have several independent failures that each need their own context chain.

## Types

| Type | Role |
| --- | --- |
| `CombinedError` | Ordered independent failures; branch holders inside an error tree |
| `ErrorReport` | Serialized snapshot of a whole tree; wire format for remote failures |
| `RemoteError` | Marker for a failure received from another process |
| `PortableError` | Error carried inside operation data rather than as the operation result |
| `TippedError` | Attaches a local and a remote Tip (`mcpm_rich::Rich`) to an error without adding a failure message |
| `contains::<T>` | Tree search that visits every branch |

## Ordinary errors

Producers build `anyhow::Error` values with `.context(...)`. Presentation renders the complete
chain, outermost first, through the existing `caused by` diagnostics entry point. Concrete error
types are retained locally and are only lost when a tree crosses a process boundary.

## `CombinedError`

```rust
use mcpm_exec::error::CombinedError;

let first = anyhow::anyhow!("publication failed");
let second = anyhow::anyhow!("rollback failed");
let error = anyhow::Error::new(CombinedError::new(first, second));
```

- `new(first, second)` requires two independent failures; `push` appends more.
- `errors()` exposes the branches in their original order.
- Branches may contain further `CombinedError` values.
- `Display` reports `N independent errors`. It implements `StdError` without a `source()`: the
  branches are siblings, not a chain, so plain chain traversal cannot reach them.

## `ErrorReport`

`ErrorReport` is the transport snapshot. It implements neither `Error` nor `Display`; it is data.

| Field | Content |
| --- | --- |
| `message` | Outermost context message; empty for a transparent aggregate root |
| `causes` | Remaining messages of the linear context chain, outermost first |
| `branches` | Independent failure subtrees; omitted from JSON when empty |
| `tips` | One `ReportTip { local, remote }` per `TippedError` in the chain, outermost first; omitted when empty |

Construction:

- `capture(&anyhow::Error)` and `capture_std(&dyn StdError)` use `Display` for every message.
- `capture_with(error, message)` maps messages: return `None` to skip a transparent wrapper.
- Capture stops the linear chain at the first `CombinedError` and captures each branch recursively.
- `RemoteError` markers are skipped while capturing, so relaying a received failure never
  duplicates the marker.
- A `TippedError` contributes a `tips` entry rather than a cause message. Below a `RemoteError`
  its remote Tip is stored as both `local` and `remote`, so the choice is final however often the
  report is forwarded. Presentation shows `local`.

Restoration with `into_error()`:

- Each branch is restored recursively; two or more branches are grouped in a new `CombinedError`.
- `message` and then `causes` are re-applied as `anyhow` context in reverse order.
- An empty message applied to an existing error is transparent; an empty message without an
  error becomes a plain message error. An empty report produces `unknown error`.
- `tips` are restored as `TippedError` layers around the innermost error, in their original order.

Round trip: capturing the restored error yields the same report. This invariant is asserted by
the module tests.

Wire shape:

```json
{
  "message": "save snapshot",
  "causes": ["publish snapshot"],
  "branches": [
    { "message": "publication failed", "causes": [] },
    { "message": "recover snapshot", "causes": [],
      "branches": [
        { "message": "cleanup failed", "causes": [] },
        { "message": "rollback failed", "causes": [] }
      ] }
  ]
}
```

`causes` and `branches` default to empty on deserialization, so reports produced before branches
existed still decode.

## `RemoteError`

`RemoteError` marks a failure the local process received from another process. Its source is the
restored diagnostic tree, so local diagnostics keep working with chain traversal. It is created
by `RemoteError::wrap(error)`.

Because the marker is not part of the original failure, `ErrorReport::capture` omits it. Use
`error::contains::<RemoteError>(error)` to detect peer failures; it traverses the whole tree,
including independent branches.

## Tips

A Tip is advice the user can act on: a command to run, a setting to change. It is not a failure
cause; the cause chain explains what failed, the Tip says what to do about it.

```rust
use mcpm_exec::error::{TipExt, TippedError};
use mcpm_rich::rich;

fn check(path: &std::path::Path, result: anyhow::Result<()>) -> anyhow::Result<()> {
    result.tip(rich!("Run `mcpm info --upgrade` to upgrade the metadata of {path:?}."))?;
    Err::<(), _>(anyhow::anyhow!("lookup failed"))
        .tips(None, Some(rich!("Only meaningful on the remote host.")))?;
    Err(TippedError::executable_not_found("git").into())
}
```

- A Tip is a `mcpm_rich::Rich` built with `rich!`; the macro's Rustdoc defines the syntax.
- `.tip(tip)` shows the same Tip everywhere. `.tips(local, remote)` separates the Tip shown in the
  process that created the error from the one shown after the error crossed a `RemoteError`
  boundary; either may be `None`.
- `TippedError::executable_not_found(name)` is the only constructor for executable lookup
  failures; it shows non-interactive SSH/PATH advice only when observed remotely.
- Mark `RemoteError` where a remote fact is first read, not where a local error is relayed. A
  wrapper is invisible to `anyhow::Error::downcast_ref`; traverse `chain()` to find typed errors
  below it.
- Tips carry no styling; frontends render roles, labels and layout.

## `PortableError`

Use `PortableError` when an item failure belongs inside an operation's output and must not fail
the whole operation, for example `Vec<Result<Entry, PortableError>>` or a failure event in a
stream.

| Situation | Behavior |
| --- | --- |
| Constructed from `anyhow::Error` locally | Original error and concrete types are kept; no marker is added |
| Serialized | Delegates to `ErrorReport` |
| Deserialized | Restores the tree and adds a `RemoteError` marker; the marker is not conditional on the execution location |
| Relayed again | Same codec; markers do not accumulate |
| Consumed | `into_inner()` returns an `anyhow::Error` for the shared diagnostics entry point |

Whole-operation failures must return `Err` from `Operation::run` or the stream producer so the
automatic transport applies.

## Transport integration

| Source | Wire form | Caller receives |
| --- | --- | --- |
| `Operation::run` returns `Err` | Reply `{"error": <report>}` | Restored tree under `RemoteError`, with context ``operation `ID` failed`` |
| Stream producer returns `Err` or panics | Final stream item `{"error": <report>}` | Restored tree under `RemoteError`, with context `remote stream failed` |
| Link startup readiness error | `Ready::Error(<report>)` | Restored tree with link startup context |
| Local execution | No serialization | Original `anyhow::Error`; no marker |

The error envelope replaces the earlier `message`/`source` wire chain, so clients, links, and
daemons must use matching builds.

## Rendering

- Render with the existing diagnostics entry point, which prints the chain as `error:` followed by
  `caused by:` lines.
- `ErrorReport` rendering, when a report is inspected directly, must recurse into `branches`;
  `anyhow::Error::chain()` alone cannot enumerate them. `CombinedError::Display` names the branch
  count instead of repeating branch messages.
- Never print `Debug` output as user-facing diagnostics.

## Producer guidelines

- Return `Err` for failures that end the operation; use `PortableError` only for per-item results.
- Add context at each boundary that knows more than its caller, not for every call.
- Do not wrap a failure already marked with `RemoteError` in a second marker; capture and restore
  handle relaying.
- Keep the outer message actionable; the causes carry the detail.
