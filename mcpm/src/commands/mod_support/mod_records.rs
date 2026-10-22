//! Display records derived from a loader's archive tree, without changing its metadata.

use kako_mods_lib::local::{
    mcmod::{ModInfo, ModMeta},
    modloader::ModLoaderMeta,
};

/// Stable positions within collect() for one loader of one file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::commands) struct RecordKey {
    pub metadata: usize,
    pub record: usize,
}

#[derive(Debug)]
pub(in crate::commands) struct ModRecord<'a> {
    pub index: String,
    pub loader: &'a ModLoaderMeta,
    pub info: &'a ModInfo,
    pub source: Vec<String>,
    pub nested_indices: Vec<usize>,
    pub native_index: Option<usize>,
}

pub(in crate::commands) fn collect<'a>(metadata: &'a ModMeta, prefix: &str) -> Vec<ModRecord<'a>> {
    let mut records = Vec::new();
    append(
        metadata,
        prefix,
        &mut Vec::new(),
        &mut Vec::new(),
        &mut records,
    );
    records
}

/// Selects each matched declaration's owning archive and all of its descendants.
pub(in crate::commands) fn selected<'a>(
    metadata: &'a ModMeta,
    metadata_index: usize,
    prefix: &str,
    keys: Option<&[RecordKey]>,
) -> Vec<ModRecord<'a>> {
    let records = collect(metadata, prefix);
    let Some(keys) = keys else {
        return records;
    };
    let roots: Vec<_> = keys
        .iter()
        .filter(|key| key.metadata == metadata_index)
        .filter_map(|key| records.get(key.record))
        .map(|record| record.nested_indices.clone())
        .collect();
    records
        .into_iter()
        .filter(|record| {
            roots
                .iter()
                .any(|root| record.nested_indices.starts_with(root))
        })
        .collect()
}

fn append<'a>(
    metadata: &'a ModMeta,
    prefix: &str,
    source: &mut Vec<String>,
    nested_indices: &mut Vec<usize>,
    records: &mut Vec<ModRecord<'a>>,
) {
    for (index, info) in metadata.mods.iter().enumerate() {
        let native_index = (metadata.mods.len() > 1).then_some(index + 1);
        let index = if metadata.mods.len() == 1 {
            prefix.to_owned()
        } else {
            format!("{prefix}-{}", index + 1)
        };
        records.push(ModRecord {
            index,
            loader: &metadata.loader,
            info,
            source: source.clone(),
            nested_indices: nested_indices.clone(),
            native_index,
        });
    }
    for (index, child) in metadata.nested.iter().enumerate() {
        source.push(child.path.clone());
        nested_indices.push(index + 1);
        append(
            &child.meta,
            &format!("{prefix}+{}", index + 1),
            source,
            nested_indices,
            records,
        );
        source.pop();
        nested_indices.pop();
    }
}
