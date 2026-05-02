//! QQ channel adapter stub — not yet ported from TypeScript.
//! The original TS source is at src-old/channels/qq.ts.

use async_trait::async_trait;

use super::{Channel, MessageCallback, MetadataCallback};

#[derive(Clone)]
pub struct QQChannel;

impl QQChannel {
    pub fn new(_app_id: String, _app_secret: String, _sandbox: bool) -> Self {
        Self
    }
}

#[async_trait]
impl Channel for QQChannel {
    fn id(&self) -> &'static str {
        "qq"
    }

    async fn connect(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn disconnect(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_connected(&self) -> bool {
        false
    }

    async fn send_message(
        &self,
        _chat_jid: &str,
        _text: &str,
        _bot_token: Option<&str>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn send_file(
        &self,
        _chat_jid: &str,
        _file_path: &str,
        _caption: Option<&str>,
        _bot_token: Option<&str>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn owns_jid(&self, _chat_jid: &str) -> bool {
        false
    }

    fn on_message(&self, _handler: MessageCallback) {}

    fn on_metadata(&self, _handler: MetadataCallback) {}
}
