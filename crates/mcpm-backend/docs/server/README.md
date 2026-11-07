# Server operations

Server operations act on the actual server directory on its owning Host. Inputs carry that
Host's path; outputs carry projections of stored data. mcpm chooses the target and renders the
result; an operation neither formats output nor reads terminal state.

| Page | Content |
| --- | --- |
| [Basics](basics.md) | Identity, recognition, initialization, and Remote preparation |
| [Stash](stash.md) | Mod snapshot creation, switching, comparison, inventory, and verification |

## Execution

- The Executor runs a Host operation locally, or on the remote Host through `mcpmd`; the operation
  does not know which happened.
- The resolved target is a Local Server. A Remote workspace is resolved by the Executor before
  the operation runs, so the operation always receives a server directory and never follows one.
- Returned paths and startup arguments keep the owning Host's platform spelling.

## Streaming

`create`, `switch`, `status`, and `verify` stream stage events followed by one business event.
Stage progress describes file work that has already happened. Dropping the stream stops production
at its next send; work already inside the library is not interrupted.

## Failures

| Channel | Use |
| --- | --- |
| `Err` from `run` or the stream producer | The whole operation failed and has no result |
| `PortableError` inside the output | One item failed; the remaining output stays valid |

Item failures travel with the item, for example one inventory record or one batch selection, so a
caller keeps successful results and still reports every failure individually. Whole-operation
failures use the automatic transport; see the [mcpm-exec error model](../../../mcpm-exec/src/docs/error.md).

Committed files are not rolled back by a lost connection. After a publication or transport
failure, the server files describe what actually happened.

[mcpm commands](../../../mcpm/docs/README.md) · [简体中文](../../docs-zh/server/README.md)
