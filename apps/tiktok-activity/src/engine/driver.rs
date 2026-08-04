//! Extension driver — drives ONE logged-in TikTok tab through the browser
//! extension over the ext-WS bridge. Replaces the earlier chromiumoxide CDP
//! driver: there is a single account (the user's real session), so there is no
//! per-account profile/proxy launch — just RPC to the extension.
//!
//! A Semaphore(1) permit is held for the whole run so two runs never fight over
//! the one tab.

use super::browser::{self, execute_action};
use super::ext_page::ExtPage;
use super::run_state::RunState;
use super::{BrowserDriver, LogFn};
use crate::bridge::Bridge;
use crate::domain::{FlowAction, TikTokAccount};
use crate::extbridge::ExtBridge;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

pub struct ExtensionDriver {
    ext: ExtBridge,
    bridge: Bridge,
    sem: Arc<Semaphore>,
    permit: Arc<Mutex<Option<OwnedSemaphorePermit>>>,
}

impl ExtensionDriver {
    pub fn new(ext: ExtBridge, bridge: Bridge) -> Self {
        Self {
            ext,
            bridge,
            sem: Arc::new(Semaphore::new(1)),
            permit: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl BrowserDriver for ExtensionDriver {
    async fn before_run(&self, _account: &TikTokAccount, log: &LogFn) -> Result<()> {
        // Serialize: only one run may drive the single tab at a time.
        let permit = self
            .sem
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| anyhow!("driver semaphore closed"))?;
        *self.permit.lock().await = Some(permit);
        if !self.ext.is_connected() {
            *self.permit.lock().await = None;
            return Err(anyhow!(
                "Chrome extension chưa kết nối — mở Chrome đã đăng nhập TikTok, cài extension tiktok-activity và mở một tab tiktok.com"
            ));
        }
        log(&"[EXT] extension connected — điều khiển 1 tab TikTok".to_string());
        Ok(())
    }

    async fn execute(
        &self,
        rs: &RunState,
        account: &TikTokAccount,
        action: &FlowAction,
        log: &LogFn,
    ) -> Result<()> {
        let page = ExtPage::new(self.ext.clone());
        execute_action(&page, rs, account, action, &self.bridge, log).await
    }

    async fn after_run(&self, _account: &TikTokAccount) {
        // Leave the user's browser open; just release the run lock.
        *self.permit.lock().await = None;
    }
}

/// Load legacy atomic rules JSON (like/follow/next_video) into the in-memory
/// book at boot, mirroring the Go main().
pub fn apply_legacy_rules_at_boot(raw: &str) {
    if raw.trim().is_empty() {
        return;
    }
    if let Err(e) = browser::apply_legacy_rules(raw) {
        tracing::warn!("legacy atomic rules apply: {e}");
    } else {
        tracing::info!("legacy atomic rules applied from SQLite");
    }
}
