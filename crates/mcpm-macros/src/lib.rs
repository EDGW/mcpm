//! Procedural macros shared by mcpm crates; each is re-exported by the crate owning its data types.

// Private Modules

mod config_fields;
mod rich;

use proc_macro::TokenStream;

/// Builds a `mcpm_rich::Rich` at compile time from a Markdown-subset literal.
///
/// Arguments follow `format!`: implicit captures, positional and `name = expr` arguments.
/// Interpolated values are data and are never parsed as markup.
///
/// # Syntax
///
/// | Source | Result |
/// |---|---|
/// | plain text | `Normal`; `Muted` inside a `> ` block |
/// | `` `code` `` | `Code` (command, path, file name, configuration key) |
/// | `[text](role)` | `primary`, `secondary`, `important`, `muted`, `success`, `warning` or `danger`; text may interpolate, may not contain code spans or nest |
/// | `{name}`, `{0}`, `{}`, `{x:?}` | interpolation; `` `{path}` `` and `[{n}](secondary)` style the value |
/// | blank line | starts a new block |
/// | single newline inside a block | joined with one space; wrapping is left to the renderer |
/// | block starting with `- ` | list; each `- ` starts an item, lines indented by 2+ spaces continue it |
/// | every line of a block starting with `> ` | unmarked text of the block becomes `Muted`; applies to paragraphs and lists |
/// | ` `` `, `{{`, `}}`, `[[`, `]]` | literal `` ` ``, `{`, `}`, `[`, `]` |
///
/// The common leading indentation and surrounding blank lines of the literal are removed, so it
/// can follow the indentation of the surrounding code. Labels such as `Tip:` belong to renderers.
///
/// # Compile errors
///
/// Unclosed `` ` ``, `{` or `[`; stray `}` or `]`; `[text]` without `(role)`; unknown, `normal`
/// or `code` role names; nested or empty markers; code spans inside markers; malformed or
/// unsupported format specs (`$`, `*`); unused or missing arguments; a block mixing `> ` and plain
/// lines; a line starting with `- ` in the middle of a paragraph; an unindented line inside a list
/// item; empty text. Errors report the line and column inside the literal.
#[proc_macro]
pub fn rich(input: TokenStream) -> TokenStream {
    rich::expand(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Exposes typed configuration leaves and their command-line shortcuts.
#[proc_macro_derive(ConfigFields, attributes(config))]
pub fn config_fields(input: TokenStream) -> TokenStream {
    syn::parse(input)
        .and_then(config_fields::expand)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
