//! Task scheduler. Port targets: src-old/scheduler/*.ts

pub mod executor;
pub mod task_scheduler;

pub use executor::DefaultTaskExecutor;
