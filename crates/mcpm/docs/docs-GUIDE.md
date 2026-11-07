# Documentation guide

If you use AI to write documentation, follow this guide.

Applies to command documentation and CLI help text under `crates/mcpm/docs`. Use `server/basics.md` as the format reference.

## Reader perspective

- Documentation serves users. Start by explaining what the command does, when to use it, and what result to expect.
- Organize content around usage. When migrating, rewrite the material instead of copying old documents or following implementation structure.
- Readers should be able to complete operations without understanding internal design.

## Structure

Organize each command in this order:

1. Purpose and use cases
2. Command form
3. Options
4. Examples
5. Behavior specific to that command
6. Behavior shared with other commands

## Section hierarchy

- Put command-specific sections under that command. For example, `launch` overrides, configuration files, and authentication belong under `mcpm launch`.
- Put sections shared by multiple commands at the end of the document, such as output modes, command input, logs, and status.
- Avoid repeating the same information in multiple sections. Link to it when needed.

## What to include and omit

Include:

- Command purpose, arguments, options, and defaults
- User-visible output, status, hints, and errors
- Operation steps, limits, and results
- Configuration file structure, path resolution, and authentication behavior
- Behavior that could be misunderstood, such as a command that appears to perform an operation but does not

Omit:

- Obvious comparisons, such as "the two commands differ"
- Repetition and information already clear from the options table
- Implementation details: internal state machines, locks, timeouts, caches, metadata, transport processes, and internal terminology
- Issues discussed during implementation and constraints unrelated to usage
- Behavior that needs no explanation, such as `remove` not adding an item

## Language

- Use formal prose. Avoid colloquial expressions.
- State behavior directly: what the command does, what the interface shows, and what happens on failure.
- Avoid consecutive negation patterns such as "does not A, does not B, and does not C." Prefer positive statements, or split limits into separate sentences.
- Keep each paragraph focused on one topic.

## Output modes

- Describe Console, Raw, TUI, and other modes by behavior, not by internal drawing or transport.
- Explain stdout, stderr, and color behavior only when users need it to understand output differences.

## CLI help text

- Group content logically. Use **bold + underline** for section headings.
- Use **bold** for referenced CLI parameters.
- Keep body text in the default style.
- Remove internal terms such as `Primary`, frame, session, shim, and frontend.
- Present content in the reader's order. Do not stack limits in short paragraphs.

## Chinese and English synchronization

- After completing the Chinese documentation, synchronize the English version and keep section structure consistent.
- After migration, delete the corresponding legacy document and update indexes at every level.
