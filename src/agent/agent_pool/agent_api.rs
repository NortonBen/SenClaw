//! AgentApi trait implementation for AgentPool.

use anyhow::Result;
use async_trait::async_trait;

use crate::agent::agent_pool::pool::AgentPool;
use crate::agent::input_builder::ImageAttachment;
use crate::types::AgentApi;
use crate::types::GroupBinding;

#[async_trait]
impl AgentApi for AgentPool {
    async fn broadcast_reply(&self, chat_jid: &str, text: &str, bot_token: Option<&str>) {
        AgentPool::broadcast_reply(self, chat_jid, text, bot_token).await
    }

    async fn process_and_wait(&self, jid: &str, group: &GroupBinding, prompt: &str) -> Result<()> {
        self.process_and_wait_inner(jid, group, prompt, 5).await
    }

    async fn process_and_wait_with_images(
        &self,
        jid: &str,
        group: &GroupBinding,
        prompt: &str,
        attachments: &[ImageAttachment],
    ) -> Result<()> {
        self.process_and_wait_inner_with_images(jid, group, prompt, attachments, 5)
            .await
    }

    async fn destroy(&self, jid: &str) {
        self.destroy_inner(jid).await;
    }

    fn get_last_reply_text(&self, jid: &str) -> Option<String> {
        self.state
            .lock()
            .unwrap()
            .last_dispatch_replies
            .get(jid)
            .cloned()
    }
}
