//! Native vanilla launch drafts using the same configuration template and validation as imports.

use anyhow::Result;

use super::{
    Arguments, ConfigurationDraft, ImportedSetting, ImportedValue, JavaEntry, LaunchConfig,
    Process, decode_conf, validate_pair,
};
use crate::local::mc::server::VANILLA_SERVER_JAR;

/// Generates a portable JAR entry and editable nogui setting without inspecting a script.
pub fn vanilla_server() -> Result<ConfigurationDraft> {
    let draft = ConfigurationDraft {
        launch: LaunchConfig {
            version: 1,
            platform: None,
            process: Process::default(),
            jvm: Arguments::default(),
            entry: JavaEntry::Jar {
                path: VANILLA_SERVER_JAR.into(),
            },
            application: Arguments::default(),
        },
        imported: vec![ImportedSetting {
            field: "server.nogui".into(),
            value: ImportedValue::Boolean(true),
        }],
    };
    let conf = decode_conf(&draft.template()?)?;
    validate_pair(&draft.launch, &conf)?;
    Ok(draft)
}
