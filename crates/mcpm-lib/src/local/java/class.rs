//! ConstantValue field inspection without decoding method bytecode.

use std::{borrow::Cow, collections::HashMap};

use anyhow::{Context, Result, ensure};
use cafebabe::{
    FieldAccessFlags, ParseOptions, attributes::AttributeData, constant_pool::LiteralConstant,
    descriptors::FieldType,
};

#[derive(Debug)]
pub(crate) enum ConstantField<'a> {
    Int(i32),
    String(Cow<'a, str>),
    /// A declared field without a supported, correctly typed ConstantValue.
    Other,
}

pub(crate) fn constant_fields<'a>(
    bytes: &'a [u8],
    names: &[&str],
) -> Result<HashMap<Cow<'a, str>, ConstantField<'a>>> {
    let mut options = ParseOptions::default();
    options.parse_bytecode(false);
    let class = cafebabe::parse_class_with_options(bytes, &options)
        .context("failed to parse Java class constant fields")?;
    let mut fields = HashMap::with_capacity(names.len());
    for field in class.fields {
        if !names.contains(&field.name.as_ref()) {
            continue;
        }
        let mut value = ConstantField::Other;
        let mut found = false;
        for attribute in field.attributes {
            if let AttributeData::ConstantValue(constant) = attribute.data
                && field.access_flags.contains(FieldAccessFlags::STATIC)
            {
                ensure!(!found, "duplicate ConstantValue for field {}", field.name);
                found = true;
                value = match (
                    field.descriptor.dimensions,
                    &field.descriptor.field_type,
                    constant,
                ) {
                    (0, FieldType::Integer, LiteralConstant::Integer(value)) => {
                        ConstantField::Int(value)
                    }
                    (0, FieldType::Object(name), LiteralConstant::String(value))
                        if &**name == "java/lang/String" =>
                    {
                        ConstantField::String(value)
                    }
                    _ => ConstantField::Other,
                };
            }
        }
        ensure!(
            !fields.contains_key(&field.name),
            "ambiguous class field {}",
            field.name
        );
        fields.insert(field.name, value);
    }
    Ok(fields)
}
