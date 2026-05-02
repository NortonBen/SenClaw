// ===== WsClient =====

use std::collections::HashSet;
use std::sync::Arc;

use axum::extract::ws::Message;

pub(crate) struct WsClient {
    pub(crate) sender: tokio::sync::mpsc::UnboundedSender<Message>,
    pub(crate) authenticated: bool,
    pub(crate) is_admin: bool,
    pub(crate) subscriptions: HashSet<String>,
}

// ===== Shared state passed through to handlers =====

pub struct WsState {
    pub config: Arc<crate::config::Config>,
    pub db: Arc<crate::db::Db>,
    pub group_manager: Arc<crate::gateway::group_manager::GroupManager>,
    pub agent_manager: Arc<crate::gateway::agent_manager::AgentManager>,
    pub binding_manager: Arc<crate::gateway::binding_manager::BindingManager>,
    pub channel_manager: Arc<crate::gateway::channel_manager::ChannelManager>,
    pub cowork_manager: Arc<crate::cowork::CoworkManager>,
    pub api: Arc<dyn super::gateway::WsGatewayApi>,
}
