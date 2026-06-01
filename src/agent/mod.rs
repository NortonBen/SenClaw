//! Agent runtime: pool, dispatch, queue, permission, persona, send/session bridges.
//! Port targets: src-old/agent/*.ts

pub mod agent_pool;
pub mod builtin_agents;
pub mod code_session;
pub mod dispatch_bridge;
pub mod group_queue;
pub mod hook_config_loader;

// Re-export commonly used hook config loader functions
pub use hook_config_loader::load_zen_hook_config;
pub mod input_builder;
pub mod isolated_runner;
pub mod permission_bridge;
pub mod persona_registry;
pub mod send_bridge;
pub mod session_bridge;
pub mod system_prompt_builder;
pub mod system_prompts;
pub mod virtual_worker_pool;
pub mod workbench_bridge;
