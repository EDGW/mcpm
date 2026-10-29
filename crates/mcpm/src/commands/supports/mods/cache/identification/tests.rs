//! Identification completion validation: source identities and request protocol.

use super::*;
use serde_json::json;

fn writing(metas: &[&str]) -> WriteArgs {
    serde_json::from_value(json!({ "metas": metas, "parallelism": 1 })).unwrap()
}

#[test]
fn duplicate_sources_are_rejected_before_storage() {
    let session = crate::commands::execution::local_session().unwrap();
    let error = match Completion::new(
        &writing(&["ident"]),
        &["modrinth".into(), "modrinth".into()],
        &session,
    ) {
        Ok(_) => panic!("duplicate sources must be rejected"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("only once"));
}

#[test]
fn cache_identity_never_contains_a_credential() {
    let session = crate::commands::execution::local_session().unwrap();
    let completion = Completion::new(
        &writing(&["ident"]),
        &["curseforge@secret".into()],
        &session,
    )
    .unwrap();
    assert_eq!(completion.keys, vec!["curseforge".to_owned()]);
}

#[test]
fn identification_requires_at_least_one_source() {
    let session = crate::commands::execution::local_session().unwrap();
    let error = match Completion::new(&writing(&["ident"]), &[], &session) {
        Ok(_) => panic!("identification must require a source"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("at least one source"));
}
