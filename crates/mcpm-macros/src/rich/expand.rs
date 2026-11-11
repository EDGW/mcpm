//! Code generation: parsed blocks to an expression that builds a `Rich` at runtime.

use std::collections::BTreeSet;

use proc_macro2::Ident;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::LitStr;

use super::{
    Input,
    parse::{Block, Piece, Reference, Span},
};

/// Binds each explicit argument once, then emits the blocks. Unused arguments are errors.
pub(crate) fn expand(input: &Input, blocks: &[Block]) -> syn::Result<TokenStream> {
    let mut used = BTreeSet::new();
    let blocks = blocks
        .iter()
        .map(|block| block_tokens(input, block, &mut used))
        .collect::<syn::Result<Vec<_>>>()?;
    for (index, argument) in input.arguments.iter().enumerate() {
        if !used.contains(&index) {
            return Err(syn::Error::new_spanned(
                &argument.expression,
                "argument is never used in the text",
            ));
        }
    }
    let bindings = input.arguments.iter().enumerate().map(|(index, argument)| {
        let binding = format_ident!("__rich_arg_{index}");
        let expression = &argument.expression;
        quote! { let #binding = &(#expression); }
    });
    Ok(quote! {
        {
            #(#bindings)*
            ::mcpm_rich::Rich::__from_blocks(::std::vec![#(#blocks),*])
        }
    })
}

fn block_tokens(
    input: &Input,
    block: &Block,
    used: &mut BTreeSet<usize>,
) -> syn::Result<TokenStream> {
    Ok(match block {
        Block::Paragraph(spans) => {
            let spans = span_list(input, spans, used)?;
            quote! { ::mcpm_rich::Block::Paragraph(#spans) }
        }
        Block::List(items) => {
            let items = items
                .iter()
                .map(|item| span_list(input, item, used))
                .collect::<syn::Result<Vec<_>>>()?;
            quote! { ::mcpm_rich::Block::List(::std::vec![#(#items),*]) }
        }
    })
}

fn span_list(
    input: &Input,
    spans: &[Span],
    used: &mut BTreeSet<usize>,
) -> syn::Result<TokenStream> {
    let spans = spans
        .iter()
        .map(|span| {
            let text = value_tokens(input, span, used)?;
            let role = Ident::new(span.role.variant(), proc_macro2::Span::call_site());
            Ok(quote! { ::mcpm_rich::Span { role: ::mcpm_rich::Role::#role, text: #text } })
        })
        .collect::<syn::Result<Vec<_>>>()?;
    Ok(quote! { ::std::vec![#(#spans),*] })
}

/// Pure literals become `String::from`; spans with placeholders become `format!`.
fn value_tokens(
    input: &Input,
    span: &Span,
    used: &mut BTreeSet<usize>,
) -> syn::Result<TokenStream> {
    if let [Piece::Literal(text)] = span.pieces.as_slice() {
        return Ok(quote! { ::std::string::String::from(#text) });
    }
    let mut format = String::new();
    for piece in &span.pieces {
        match piece {
            Piece::Literal(text) => format.push_str(&text.replace('{', "{{").replace('}', "}}")),
            Piece::Placeholder { reference, spec } => {
                let target = resolve(input, reference, used)?;
                format.push('{');
                format.push_str(&target);
                if !spec.is_empty() {
                    format.push(':');
                    format.push_str(spec);
                }
                format.push('}');
            }
        }
    }
    // The literal's span lets implicit captures resolve to variables at the call site.
    let format = LitStr::new(&format, input.literal.span());
    Ok(quote! { ::std::format!(#format) })
}

/// Explicit arguments map to their bound local; unknown names stay implicit captures.
fn resolve(
    input: &Input,
    reference: &Reference,
    used: &mut BTreeSet<usize>,
) -> syn::Result<String> {
    let index = match reference {
        Reference::Index(index) => {
            let positional = input
                .arguments
                .iter()
                .take_while(|argument| argument.name.is_none())
                .count();
            if *index >= positional {
                return Err(syn::Error::new(
                    input.literal.span(),
                    format!(
                        "rich! refers to positional argument {index}, but there are {positional}"
                    ),
                ));
            }
            *index
        }
        Reference::Name(name) => match input
            .arguments
            .iter()
            .position(|argument| argument.name.as_ref().is_some_and(|ident| ident == name))
        {
            Some(index) => index,
            None => return Ok(name.clone()),
        },
    };
    used.insert(index);
    Ok(format!("__rich_arg_{index}"))
}
