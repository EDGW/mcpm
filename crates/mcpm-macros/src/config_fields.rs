//! Derives typed configuration paths and shortcut declarations from Serde field names.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Data, DeriveInput, Fields, GenericArgument, LitStr, Meta, PathArguments, Token, Type,
    punctuated::Punctuated,
};

fn serde_string(attrs: &[syn::Attribute], key: &str) -> syn::Result<Option<String>> {
    let mut result = None;
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        for meta in attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)? {
            if !meta.path().is_ident(key) {
                continue;
            }
            if let Meta::NameValue(value) = &meta
                && let syn::Expr::Lit(expr) = &value.value
                && let syn::Lit::Str(value) = &expr.lit
            {
                result = Some(value.value());
            } else {
                return Err(syn::Error::new_spanned(
                    meta,
                    "expected one shared Serde name",
                ));
            }
        }
    }
    Ok(result)
}

fn canonical_name(name: &str, rule: Option<&str>) -> syn::Result<String> {
    let name = name.strip_prefix("r#").unwrap_or(name);
    match rule {
        None => Ok(name.to_owned()),
        Some("camelCase") => {
            let mut camel = String::new();
            let mut capitalize = false;
            for ch in name.trim_start_matches('_').chars() {
                if ch == '_' {
                    capitalize = true;
                } else if camel.is_empty() {
                    camel.push(ch.to_ascii_lowercase());
                } else if capitalize {
                    camel.push(ch.to_ascii_uppercase());
                    capitalize = false;
                } else {
                    camel.push(ch);
                }
            }
            Ok(camel)
        }
        Some(rule) => Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("unsupported Serde rename_all rule {rule:?}"),
        )),
    }
}

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let rule = serde_string(&input.attrs, "rename_all")?;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "ConfigFields requires a named struct",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "ConfigFields requires a named struct",
        ));
    };
    let mut lookup = Vec::new();
    let mut apply = Vec::new();
    let mut visit = Vec::new();
    for field in &fields.named {
        let member = field.ident.as_ref().unwrap();
        let ty = &field.ty;
        let name = serde_string(&field.attrs, "rename")?
            .unwrap_or(canonical_name(&member.to_string(), rule.as_deref())?);
        let mut nested = false;
        let mut command: Option<LitStr> = None;
        let mut short: Option<syn::LitChar> = None;
        for attr in field
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("config"))
        {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("nested") {
                    nested = true;
                } else if meta.path.is_ident("name") {
                    let explicit: LitStr = meta.value()?.parse()?;
                    if explicit.value() != name {
                        return Err(syn::Error::new_spanned(
                            explicit,
                            format!("config name must match canonical Serde name {name:?}"),
                        ));
                    }
                } else if meta.path.is_ident("command") {
                    command = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("command_short") {
                    short = Some(meta.value()?.parse()?);
                } else {
                    return Err(meta.error("unknown config attribute"));
                }
                Ok(())
            })?;
        }
        let help = field
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("doc"))
            .filter_map(|attr| {
                if let Meta::NameValue(value) = &attr.meta
                    && let syn::Expr::Lit(expr) = &value.value
                    && let syn::Lit::Str(value) = &expr.lit
                {
                    return Some(value.value().trim().to_owned());
                }
                None
            })
            .collect::<Vec<_>>()
            .join(" ");
        if nested {
            if command.is_some() || short.is_some() {
                return Err(syn::Error::new_spanned(
                    field,
                    "nested fields cannot declare shortcuts",
                ));
            }
            let optional = if let Type::Path(path) = ty {
                path.path
                    .segments
                    .last()
                    .filter(|segment| segment.ident == "Option")
                    .and_then(|segment| {
                        if let PathArguments::AngleBracketed(args) = &segment.arguments
                            && let Some(GenericArgument::Type(inner)) = args.args.first()
                        {
                            return Some(inner);
                        }
                        None
                    })
            } else {
                None
            };
            let inner = optional.unwrap_or(ty);
            let target = if optional.is_some() {
                quote!(self.#member.get_or_insert_with(::std::default::Default::default))
            } else {
                quote!(&mut self.#member)
            };
            lookup.push(quote! { if let Some(rest) = path.strip_prefix(concat!(#name, ".")) { return <#inner as super::overrides::ConfigFields>::field(rest); } });
            apply.push(quote! { if let Some(rest) = path.strip_prefix(concat!(#name, ".")) { return <#inner as super::overrides::ConfigFields>::apply_field(#target, rest, value, mode); } });
            visit.push(quote! { prefix.push(#name); <#inner as super::overrides::ConfigFields>::visit_shortcuts(prefix, visitor); prefix.pop(); });
        } else {
            let has_shortcut = command.is_some() || short.is_some();
            let command = command
                .map(|value| quote!(Some(#value)))
                .unwrap_or(quote!(None));
            let short = short
                .map(|value| quote!(Some(#value)))
                .unwrap_or(quote!(None));
            let descriptor = quote!(super::overrides::Field::of::<#ty>(#command, #short, #help));
            lookup.push(quote! { if path == #name { return Some(#descriptor); } });
            apply.push(quote! { if path == #name { return super::overrides::FieldValue::apply(&mut self.#member, value, mode); } });
            if has_shortcut {
                visit.push(
                    quote! { prefix.push(#name); visitor(prefix, #descriptor); prefix.pop(); },
                );
            }
        }
    }
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics super::overrides::ConfigFields for #ident #ty_generics #where_clause {
            fn field(path: &str) -> Option<super::overrides::Field> {
                #(#lookup)*
                None
            }
            fn apply_field(&mut self, path: &str, value: &::toml::Value, mode: super::overrides::Mode) -> ::anyhow::Result<()> {
                #(#apply)*
                Err(::anyhow::anyhow!("unknown configuration field {:?} in {}", path, stringify!(#ident)))
            }
            fn visit_shortcuts(prefix: &mut Vec<&'static str>, visitor: &mut dyn FnMut(&[&'static str], super::overrides::Field)) {
                let _ = (&prefix, &visitor);
                #(#visit)*
            }
        }
    })
}
