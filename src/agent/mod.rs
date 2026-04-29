//! Agent runtime: pool, dispatch, queue, permission, persona, send/session bridges.
//! Port targets: src-old/agent/*.ts

pub mod agent_pool;
pub mod dispatch_bridge;
pub mod group_queue;
pub mod permission_bridge;
pub mod persona_registry;
pub mod send_bridge;
pub mod session_bridge;
pub mod virtual_worker_pool;
