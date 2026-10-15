use std::ffi::OsString;

use super::LaunchPlan;

impl LaunchPlan {
    /// Diagnostic argument view, with identity credentials and common credential
    /// switches/properties redacted. Execution always uses the original OsStrings.
    /// Arbitrary application-specific secrets should be kept out of arguments.
    pub fn redacted_arguments(&self) -> Vec<OsString> {
        let identity = &self.options.identity;
        let mut secrets = vec![
            identity.access_token.expose(),
            identity.xuid.as_deref().unwrap_or_default(),
            identity.client_id.as_deref().unwrap_or_default(),
        ];
        secrets.retain(|value| !value.is_empty() && *value != "0");
        secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
        let mut hide_next = false;
        self.arguments()
            .into_iter()
            .map(|value| {
                let value = value.to_string_lossy();
                let (key, inline) = value
                    .split_once('=')
                    .map_or((value.as_ref(), None), |(key, value)| (key, Some(value)));
                let sensitive = sensitive_key(key);
                let next = sensitive && inline.is_none();
                let mut redacted = if hide_next {
                    "<redacted>".to_owned()
                } else if sensitive && inline.is_some() {
                    format!("{key}=<redacted>")
                } else {
                    value.into_owned()
                };
                hide_next = next;
                for secret in &secrets {
                    redacted = redacted.replace(secret, "<redacted>");
                }
                redacted.into()
            })
            .collect()
    }
}

fn sensitive_key(key: &str) -> bool {
    if !key.starts_with('-') {
        return false;
    }
    let key: String = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect();
    [
        "accesstoken",
        "authsession",
        "sessionid",
        "password",
        "secret",
        "userproperties",
        "clientid",
        "xuid",
    ]
    .iter()
    .any(|suffix| key.ends_with(suffix))
        || matches!(key.as_str(), "token" | "session")
}
