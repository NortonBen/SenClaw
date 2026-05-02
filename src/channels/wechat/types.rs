//! JSON types for WeChat iLink Bot API.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WeixinAccountData {
    pub(crate) token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) base_url: Option<String>,
    #[serde(rename = "userId", skip_serializing_if = "Option::is_none")]
    pub(crate) user_id: Option<String>,
    #[serde(rename = "savedAt")]
    pub(crate) saved_at: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SendMessageRequest {
    pub(crate) msg: SendMessageMsg,
}

#[derive(Debug, Serialize)]
pub(crate) struct SendMessageMsg {
    pub(crate) from_user_id: String,
    pub(crate) to_user_id: String,
    pub(crate) client_id: String,
    pub(crate) message_type: u32,
    pub(crate) message_state: u32,
    pub(crate) item_list: Vec<MessageItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) context_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MessageItem {
    #[serde(rename = "type")]
    pub(crate) item_type: u32,
    pub(crate) text_item: Option<TextItem>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TextItem {
    pub(crate) text: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GetUpdatesResponse {
    pub(crate) ret: Option<i32>,
    pub(crate) errcode: Option<i32>,
    pub(crate) errmsg: Option<String>,
    pub(crate) msgs: Option<Vec<WeixinMessage>>,
    pub(crate) get_updates_buf: Option<String>,
    pub(crate) longpolling_timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WeixinMessage {
    #[serde(rename = "message_id")]
    pub(crate) message_id: Option<u64>,
    pub(crate) from_user_id: Option<String>,
    pub(crate) to_user_id: Option<String>,
    pub(crate) create_time_ms: Option<i64>,
    pub(crate) message_type: Option<u32>,
    pub(crate) item_list: Option<Vec<WeixinMessageItem>>,
    pub(crate) context_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WeixinMessageItem {
    #[serde(rename = "type")]
    pub(crate) item_type: Option<u32>,
    pub(crate) text_item: Option<WeixinTextItem>,
    pub(crate) voice_item: Option<WeixinTextItem>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WeixinTextItem {
    pub(crate) text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct QrCodeResponse {
    pub(crate) qrcode: String,
    pub(crate) qrcode_img_content: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct QrStatusResponse {
    pub(crate) status: String,
    pub(crate) bot_token: Option<String>,
    pub(crate) ilink_bot_id: Option<String>,
    pub(crate) baseurl: Option<String>,
    pub(crate) ilink_user_id: Option<String>,
}
