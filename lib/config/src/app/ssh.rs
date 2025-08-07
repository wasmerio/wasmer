use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Configure SSH server credentials and settings.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct CapabilitySshServerV1 {
    /// Enable an SSH server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub users: Option<Vec<SshUserV1>>,

    /// Additional unknown fields.
    /// This provides a small bit of forwards compatibility.
    #[serde(flatten)]
    pub other: IndexMap<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Clone, Debug)]
pub struct SshUserV1 {
    /// The username used for SSH login.
    pub username: String,

    /// Passwords for this user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passwords: Option<Vec<PasswordV1>>,

    /// SSH public keys for this user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorized_keys: Option<Vec<String>>,

    /// Additional unknown fields.
    /// This provides a small bit of forwards compatibility.
    #[serde(flatten)]
    pub other: IndexMap<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, JsonSchema, PartialEq, Eq, Clone, Debug)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum PasswordV1 {
    /// Plain text password.
    Plain { password: String },
    /// Bcrypt password hash.
    Bcrypt { hash: String },
}
