//! Gateway: HTTP/WS UI server, message router, group manager, command dispatch, triggers.
//! Port targets: src-old/gateway/*.ts

pub mod command_dispatcher;
pub mod group_manager;
pub mod message_router;
pub mod trigger_checker;
pub mod ui_server;
pub mod websocket_gateway;
