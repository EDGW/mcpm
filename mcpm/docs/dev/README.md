# Development principles

## 1. Commands

Commands are classified by reuse as **single entry point commands** or **shared commands**.

### 1.1. Single entry point commands

A single entry point command is called from only one command path. For example,
`mcpm server init` exists only under `server`, so its implementation and entry
point belong in `src/commands/server/init.rs`. The source path should reflect the
command path.

If the command also supports Remote execution, place its Remote entry point and
definition next to the command entry point.

### 1.2. Shared commands

A shared command has multiple entry points. For example, `mcpm mod list` can be
called directly or through `mcpm server mods list`, with some differences in
behavior. The `mod` command family corresponds to both `mcpm mod` and
`mcpm server mods`; its shared implementation belongs in
`src/commands/supports/mod/`, with internal paths that reflect the command path.
Place its Remote entry point alongside the shared implementation.

The Remote interface differs from the CLI interface. Remote generally exposes
the underlying operation: both `mcpm mod list` and `mcpm server mods list` use
one `mod.list` Remote command, passing different arguments to it.

Therefore, the Remote entry point belongs at the common `supports` entry point,
not at either CLI entry point. Every caller of a shared command must use that
entry point instead of maintaining its own implementation.

## 2. Arguments

Arguments can be complex. Each command usually has a primary argument.

### 2.1. Primary arguments

Query commands should generally accept multiple inputs and one input per line
from `stdin`. For example, `mcpm cache mods info <KEYS>` should accept multiple
keys from both `<KEYS>` and `stdin`.

#### 2.1.1. Primary argument types

When a key can have several types, as with `mcpm mod info` accepting a mod ID,
filename, or name:

- Use `--input <type>` to interpret `stdin`.
- Use `--<specific-type> <value>` to specify an input type explicitly.

**Decide whether each command supports automatic type inference when designing it.**

- If it does, `type` must include `unspecified` as the default.
- If it does not, nonempty `stdin` must require `--input`, or have an explicit
  default agreed on by the developers. With a default, interpret positional
  `<KEYS>` using that default. Without a default, reject positional `<KEYS>` and
  require `--<specific-type> <value>`.

**Example with automatic type inference:**

```plaintext
> mcpm mod info --help
...
      --file <FILE>          Inspect a file without falling back to modid or name; repeat to select more
      --modid <MODID>        Match an exact modid; repeat to select more
      --name <NAME>          Match an exact mod display name; repeat to select more
      --input <TYPE>         Interpret stdin lines as names, files, modids, or automatic filename/modid/name queries [default: unspecified] [possible values: name, file, modid, unspecified]
...
```

These rules are unnecessary when the primary argument has only one type, such
as a BLAKE3 key in the cache commands.

### 2.2. Other arguments

#### 2.2.1. List arguments

Represent a list with both `--<plural> A,B,C` and repeated singular options,
such as `--<singular> A --<singular> B`.

Some arguments are a **single choice**, not a list, and do not follow this rule.
For example, `--source` in `mcpm mod source search` selects one source. In
`cache mods refresh <KEYS> ident`, `--source` and `--sources` form a list and
follow this section.

**Standard example:**

```plaintext
  -l, --loader <LOADER>      Select loaders, optionally comma-separated; repeat to select more [possible values: forge, neoforge, fabric]
  -L, --loaders <LOADERS>    Select comma-separated loaders; combined with all --loader options [possible values: forge, neoforge, fabric]
```

The final Loader set combines both options.

**Rules for defaults:**

- **The default is the full set:** Any explicit selection replaces all defaults.
- **The default is empty:** Use only the explicitly selected values.
- **The default is a subset:** The semantics depend on the argument. An AI
  working on such an argument **must discuss its semantics with the developer**.

If the default is nonempty but an empty set is meaningful, add `--no-<plural>`.
It conflicts with both `--<plural>` and `--<singular>`.

If the default is a subset and the developer decides explicit values should
*add to* it, add `--no-default-<plural>`. That flag does not conflict with
`--<plural>` or `--<singular>`.

When the empty set is allowed, `--<plural> ','` is also a valid way to select it.

#### 2.2.2. Loader lists

The full Loader set is `neoforge,fabric,forge`. Loader lists generally default
to the full set and do not allow the empty set.

#### 2.2.3. Source lists

The built-in sources are `modrinth,curseforge`. Source lists generally default
to both, reject the empty set, and replace the defaults on explicit selection.

#### 2.2.4. Column lists

Columns control output fields and may have a default subset. Implement them as
follows:

- Explicit columns **replace**, rather than extend, the defaults.
- An empty selection is invalid.
- Provide a compact form, usually with different argument syntax.

**Standard column example:**

```plaintext
  -C <INMFL>          Select columns by letters: I=index, N=name, M=modid, F=filename, L=loaders

  --columns <COLUMNS> Select comma-separated columns, combined with -C and --column
                      [possible values: index, name, modid, filename, loaders]

  --column <COLUMN>   Select one column; repeat to select multiple columns
                      [possible values: index, name, modid, filename, loaders]
```

`-C` accepts single-letter column names without commas.

## 3. Output

Output has three modes: `Raw`, `Console`, and `TUI`. A command may offer several
modes and must list its supported modes in `--help`.

**Every command must support Console and Raw.**

Use `--raw` for Raw. Commands with a TUI may use `--no-ui` for Console;
`--no-ui --raw` is equivalent to `--raw`.

By default, use the first available mode. Automatically select Raw when
`stdout` is not a TTY. Use `stderr` detection for progress output, and send
progress bars to `stderr` by default. Do not use `stdin` for mode detection.

### 3.1. Raw mode

Raw generally emits TSV for automation, one item per line. JSON may be used
when data is hierarchical or contains tabs or newlines and cannot be expressed
as a simple table. An AI must discuss that choice with the developer first.

Raw omits headers by default. `--headers` enables them only in Raw. Raw should
usually emit only the command's primary data.

For example, the primary data of `mcpm cache mods list` includes a cache BLAKE3
hash, filename, size, SHA-256, and metadata names. A row could look like:

```plaintext
<blake3>\t<filename>\t<sha256>\tident,meta
```

### 3.2. Console mode

Console uses the Primary and Secondary colors to make output easy to read.
It does not need TSV and may show extra data so users need fewer commands.

For example, `mcpm cache mods list` can show metadata details even though its
primary data consists of BLAKE3, filename, size, SHA-256, and metadata names:

```plaintext
<BLAKE3>  available
  File: <file>
  Size: xxx bytes
  SHA-256: <sha256>
  2 Metas:
    ident:
    ...
    meta:
    ...
```

Showing each Meta here lets users inspect it without another command.

### 3.3. TUI mode

TODO.

### 3.4. Rendering shared data

Each data type should generally have one output format across Raw and Console.
For example, `ModMeta` and `CachedModMeta` should be rendered consistently
wherever they appear. Put shared rendering code in `src/display/xxx`.

Report errors through `diagnostics::error`; use `Progress::report` when a
progress bar is active. The diagnostics entry point owns logging, remote error
marking, and progress coordination. `src/display/error.rs` renders `error:` or
`remote error:` and each `caused by:` layer. Preserve error context; do not
print errors directly or collapse the chain into one line.

Console renderers use `ConsoleWriter` from `src/display/console`:

- Wrap the output stream with `ConsoleWriter::new` at the Console entry point;
  child renderers receive the existing writer.
- Call `subwriter()` for the next indentation level. The single `INDENT`
  constant defines each level as two spaces; renderers do not handwrite it.
- A child writer borrows its parent and can create further children. The
  parent can resume writing after the borrow ends.
- Use standard `write!`, `writeln!`, or `std::io::Write`. Line state persists
  across writes, and empty lines get no indentation.
- Raw writes directly to the original output stream.

```rust
let mut console = ConsoleWriter::new(output);
writeln!(console, "Metadata:")?;
{
    let mut metadata = console.subwriter();
    writeln!(metadata, "meta:")?;
    let mut details = metadata.subwriter();
    writeln!(details, "Status: available")?;
}
writeln!(console, "Next entry")?;
```

## 4. Common command types

### 4.1. `info`

`info` commands generally support multiple primary inputs from both arguments
and `stdin`, following the rules above. An `info` command with complex output
should also support columns.

### 4.2. `list`

Unlike `info`, a `list` command generally has no primary argument. It should
usually support columns, with Raw output suitable for automation.

A tree-shaped list, such as one containing Mods and Nested Mods, may indicate
collapsed children (for example, `(+x mods)` in `mod list`). Leave children
collapsed by default and offer `--expanded` to show them. Commands without
collapsible data do not need `--expanded`; use `--details` for ordinary details.

**Console list behavior:**

- **Default:** Show only the most important columns, such as cache BLAKE3 or
  ModName + ModId. If few columns are shown and the list is not expanded,
  multiple items may share a line without a header, as in `ls`.
- **`--details`:** Select every column by default, without enabling expansion.
- **`--columns`:** Explicit columns require one item per row and a header.
- Console can use a table or labeled field groups such as `Name: value`.
  Separate field groups with a blank line. If each group contains only one
  field, place its optional header once above the list, not above every item;
  no blank lines are needed between items.
- Above 80 items, ask whether the user wants to display the list using `Y/n`.
- Do not provide `--expanded` when there is nothing collapsible.

Examples:

```plaintext
# Detailed table
Name    Id      File
<name>  <id>    <file>
<name>  <id>    <file>
<name>  <id>    <file>

# Detailed field groups

Name: <name>
Id: <id>
File: <file>

Name: <name>
Id: <id>
File: <file>

Name: <name>
Id: <id>
File: <file>

# Single-column list or field groups, case A (short values)
<name>  <name>  <name>  <name>
<name>  <name>  <name>

# Single-column list or field groups, case B (long values)
Header
<name>
<name>
<name>
<name>
<name>
```
