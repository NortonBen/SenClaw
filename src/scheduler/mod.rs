//! Task scheduler. Port targets: src-old/scheduler/*.ts

pub mod event_notifier;
pub mod executor;
pub mod task_scheduler;

pub use event_notifier::{EventNotifier, EventNotifySink, NoopEventSink};
pub use executor::DefaultTaskExecutor;
