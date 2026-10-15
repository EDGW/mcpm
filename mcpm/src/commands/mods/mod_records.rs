//! Display records derived from a loader's archive tree, without changing its metadata.

use kako_mods_lib::local::{
    mcmod::{ModInfo, ModMeta},
    modloader::ModLoaderMeta,
};

pub(super) struct ModRecord<'a> {
    pub loader: &'a ModLoaderMeta,
    pub info: &'a ModInfo,
    pub source: Vec<String>,
}

pub(super) fn collect(metadata: &ModMeta) -> Vec<ModRecord<'_>> {
    let mut records = Vec::new();
    append(metadata, &mut Vec::new(), &mut records);
    records
}

fn append<'a>(metadata: &'a ModMeta, source: &mut Vec<String>, records: &mut Vec<ModRecord<'a>>) {
    for info in &metadata.mods {
        records.push(ModRecord {
            loader: &metadata.loader,
            info,
            source: source.clone(),
        });
    }
    for child in &metadata.nested {
        source.push(child.path.clone());
        append(&child.meta, source, records);
        source.pop();
    }
}
