//! Implementation of the `rich!` macro: argument parsing and the pipeline from literal to tokens.

// Modules

mod expand;
mod parse;

use proc_macro2::TokenStream;
use syn::{
    Expr, Ident, LitStr, Token,
    parse::{Parse, ParseStream},
};

/// Expands `rich!("literal", args...)` into an expression building a `Rich`.
pub(crate) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let input: Input = syn::parse2(input)?;
    let blocks = parse::parse(&input.literal.value()).map_err(|error| {
        syn::Error::new(
            input.literal.span(),
            format!(
                "rich!: {} (line {}, column {} of the literal)",
                error.message, error.pos.line, error.pos.column
            ),
        )
    })?;
    expand::expand(&input, &blocks)
}

/// Macro input with `format!` argument rules: positional arguments precede named ones.
pub(crate) struct Input {
    pub(crate) literal: LitStr,
    pub(crate) arguments: Vec<Argument>,
}

pub(crate) struct Argument {
    pub(crate) name: Option<Ident>,
    pub(crate) expression: Expr,
}

impl Parse for Input {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let literal: LitStr = input.parse()?;
        let mut arguments: Vec<Argument> = Vec::new();
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let name = if input.peek(Ident) && input.peek2(Token![=]) {
                let name: Ident = input.parse()?;
                input.parse::<Token![=]>()?;
                Some(name)
            } else {
                None
            };
            let expression: Expr = input.parse()?;
            if name.is_none() && arguments.iter().any(|argument| argument.name.is_some()) {
                return Err(syn::Error::new_spanned(
                    &expression,
                    "positional arguments cannot follow named arguments",
                ));
            }
            arguments.push(Argument { name, expression });
        }
        Ok(Self { literal, arguments })
    }
}
