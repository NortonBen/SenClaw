use serde::{Deserialize, Serialize};

/// A configured IMAP/SMTP account (password held separately in the DB).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub label: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: i64,
    pub smtp_host: String,
    pub smtp_port: i64,
    pub use_tls: bool,
    pub created_at: i64,
}

/// Full account row including the (currently plaintext-prefixed) password.
/// Used internally by the mailer; never serialized to the client.
#[derive(Debug, Clone)]
pub struct AccountSecret {
    pub id: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: i64,
    pub smtp_host: String,
    pub smtp_port: i64,
    pub username: String,
    pub password: String,
    pub use_tls: bool,
}

impl AccountSecret {
    /// Strip the `plaintext:` storage prefix used for parity with senclaw core.
    pub fn plain_password(&self) -> String {
        self.password
            .strip_prefix("plaintext:")
            .unwrap_or(&self.password)
            .to_string()
    }
}

#[derive(Debug, Deserialize)]
pub struct AccountCreate {
    pub label: String,
    pub email: String,
    pub imap_host: String,
    #[serde(default = "default_imap_port")]
    pub imap_port: i64,
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: i64,
    pub username: String,
    pub password: String,
    #[serde(default = "default_true")]
    pub use_tls: bool,
}

pub fn default_imap_port() -> i64 {
    993
}
pub fn default_smtp_port() -> i64 {
    587
}
pub fn default_true() -> bool {
    true
}
