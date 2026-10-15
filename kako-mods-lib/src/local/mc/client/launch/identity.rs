use std::fmt;

use md5::{Digest, Md5};
use serde_json::Value;

/// Caller-supplied identity. No account login or token refresh is performed.
/// Debug intentionally omits credentials and user properties.
#[derive(Clone)]
pub struct Identity {
    pub player_name: String,
    /// Dashed or undashed UUID; normalized to 32 hexadecimal digits when planning.
    pub uuid: String,
    pub access_token: Secret,
    pub user_type: String,
    pub xuid: Option<String>,
    pub client_id: Option<String>,
    pub user_properties: Value,
}

impl Identity {
    /// Java's UUID.nameUUIDFromBytes("OfflinePlayer:" + name), encoded as UTF-8.
    pub fn offline(player_name: impl Into<String>) -> Self {
        let player_name = player_name.into();
        let mut bytes: [u8; 16] =
            Md5::digest(format!("OfflinePlayer:{player_name}").as_bytes()).into();
        bytes[6] = (bytes[6] & 0x0f) | 0x30;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let uuid = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        Self {
            player_name,
            uuid,
            access_token: Secret::new("0"),
            user_type: "legacy".into(),
            xuid: None,
            client_id: None,
            user_properties: serde_json::json!({}),
        }
    }

    pub fn online(
        player_name: impl Into<String>,
        uuid: impl Into<String>,
        access_token: impl Into<String>,
    ) -> Self {
        Self {
            player_name: player_name.into(),
            uuid: uuid.into(),
            access_token: Secret::new(access_token),
            user_type: "msa".into(),
            xuid: None,
            client_id: None,
            user_properties: serde_json::json!({}),
        }
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identity")
            .field("player_name", &self.player_name)
            .field("uuid", &self.uuid)
            .field("user_type", &self.user_type)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}
