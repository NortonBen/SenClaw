//! Workbench — artifact publishing surface for tools/agents.
//!
//! Mirrors the sema-core TS `WorkbenchService` API. Three artifact modes:
//!   - **Static**: bundle of files (html/markdown/code) rendered by the WebUI
//!   - **Web**: external URL (e.g. running React app) embedded in iframe
//!   - **Backend**: running service with a URL + usage notes (no iframe)
//!
//! State lives in-memory per `ZenEngine`. The WorkbenchBridge subscribes to
//! [`crate::zen_core::EngineEvent::WorkbenchNew`] / `WorkbenchService*` events
//! and forwards them to the WebSocket gateway and IM fallbacks.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::events::EngineEvent;
use super::EventBus;

/// Artifact mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkbenchMode {
    /// File bundle (rendered by WebUI: HTML iframe, Markdown view, etc.).
    Static,
    /// Live web app at `url` — embedded in iframe.
    Web,
    /// Backend service — `url` + Markdown usage notes; no iframe.
    Backend,
}

impl WorkbenchMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkbenchMode::Static => "static",
            WorkbenchMode::Web => "web",
            WorkbenchMode::Backend => "backend",
        }
    }
}

/// A file inside a static artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchFile {
    pub path: String,
    /// Optional inline content. When `None`, the WebUI must call
    /// [`WorkbenchService::read_file`] to fetch from disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Service-mode process metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    /// "starting" | "ready" | "crashed" | "stopped"
    pub status: String,
    #[serde(rename = "logPath", skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
}

/// A workbench artifact. Sent to UI via `workbench:new`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchArtifact {
    pub id: String,
    pub title: String,
    pub mode: WorkbenchMode,
    /// Files for `Static` mode.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<WorkbenchFile>,
    /// URL for `Web` / `Backend` modes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Process state (Web/Backend only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process: Option<ProcessInfo>,
    /// Markdown usage notes (Backend mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<String>,
    /// Filesystem root for static-mode `read_file`. Not sent to UI.
    #[serde(skip)]
    pub root_dir: Option<PathBuf>,
    /// Unix-millis creation timestamp.
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Reason a service artifact stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Manual,
    Idle,
    SessionEnd,
}

impl StopReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            StopReason::Manual => "manual",
            StopReason::Idle => "idle",
            StopReason::SessionEnd => "session_end",
        }
    }
}

// ============================================================================
// Event data types
// ============================================================================

/// Event: `workbench:new`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchNewData {
    pub artifact: WorkbenchArtifact,
    /// If non-None, this artifact replaces an existing artifact with the given id.
    #[serde(rename = "replacesId", skip_serializing_if = "Option::is_none")]
    pub replaces_id: Option<String>,
}

/// Event: `workbench:service_ready`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchServiceReadyData {
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    pub ready: bool,
}

/// Event: `workbench:service_crashed`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchServiceCrashedData {
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    #[serde(rename = "lastLogLines")]
    pub last_log_lines: String,
}

/// Event: `workbench:service_stopped`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchServiceStoppedData {
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    pub reason: StopReason,
}

// ============================================================================
// WorkbenchService — engine-side state + reverse ops
// ============================================================================

/// Per-engine workbench state. Tracks live artifacts, viewed status, and
/// emits engine events on publish / lifecycle changes.
pub struct WorkbenchService {
    event_bus: EventBus,
    artifacts: Mutex<HashMap<String, WorkbenchArtifact>>,
    /// Artifact IDs the user has viewed at least once.
    viewed: Mutex<HashSet<String>>,
    /// Artifact IDs the user has closed.
    closed: Mutex<HashSet<String>>,
}

impl WorkbenchService {
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            event_bus,
            artifacts: Mutex::new(HashMap::new()),
            viewed: Mutex::new(HashSet::new()),
            closed: Mutex::new(HashSet::new()),
        }
    }

    /// Publish a new artifact. Optionally replaces an existing artifact id.
    /// Emits `workbench:new`.
    pub fn publish(
        &self,
        artifact: WorkbenchArtifact,
        replaces_id: Option<String>,
    ) -> WorkbenchArtifact {
        if let Some(ref rid) = replaces_id {
            self.artifacts.lock().unwrap().remove(rid);
        }
        self.artifacts
            .lock()
            .unwrap()
            .insert(artifact.id.clone(), artifact.clone());

        self.event_bus.emit(EngineEvent::WorkbenchNew(WorkbenchNewData {
            artifact: artifact.clone(),
            replaces_id,
        }));
        artifact
    }

    /// Mark an artifact as viewed. Returns `true` when the artifact exists.
    pub fn mark_viewed(&self, artifact_id: &str) -> bool {
        let exists = self.artifacts.lock().unwrap().contains_key(artifact_id);
        if exists {
            self.viewed.lock().unwrap().insert(artifact_id.to_string());
        }
        exists
    }

    /// Close an artifact. For service mode, callers should stop the process
    /// first via [`Self::notify_service_stopped`]. Emits no event by itself —
    /// the bridge / UI tracks closures via the response.
    pub fn close(&self, artifact_id: &str) -> bool {
        let removed = self.artifacts.lock().unwrap().remove(artifact_id).is_some();
        if removed {
            self.closed.lock().unwrap().insert(artifact_id.to_string());
        }
        removed
    }

    /// List current artifacts.
    pub fn list(&self) -> Vec<WorkbenchArtifact> {
        self.artifacts.lock().unwrap().values().cloned().collect()
    }

    pub fn get(&self, artifact_id: &str) -> Option<WorkbenchArtifact> {
        self.artifacts.lock().unwrap().get(artifact_id).cloned()
    }

    /// Read a file from a static artifact's `root_dir`. Returns
    /// `Err("not_found" | "no_root" | "outside_root" | io_msg)`.
    pub fn read_file(&self, artifact_id: &str, file_path: &str) -> Result<String, String> {
        let artifact = self
            .artifacts
            .lock()
            .unwrap()
            .get(artifact_id)
            .cloned()
            .ok_or_else(|| "not_found".to_string())?;

        if let Some(file) = artifact.files.iter().find(|f| f.path == file_path) {
            if let Some(ref content) = file.content {
                return Ok(content.clone());
            }
        }

        let root = artifact.root_dir.as_deref().ok_or_else(|| "no_root".to_string())?;
        let resolved = root.join(file_path);
        let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
        let canonical = resolved.canonicalize().map_err(|e| e.to_string())?;
        if !canonical.starts_with(&canonical_root) {
            return Err("outside_root".to_string());
        }
        std::fs::read_to_string(&canonical).map_err(|e| e.to_string())
    }

    /// Tail the artifact's log file (service mode). Returns "" when no log.
    pub fn fetch_logs(&self, artifact_id: &str, tail_lines: usize) -> String {
        let log_path: Option<String> = self
            .artifacts
            .lock()
            .unwrap()
            .get(artifact_id)
            .and_then(|a| a.process.as_ref()?.log_path.clone());
        let Some(path) = log_path else {
            return String::new();
        };
        match std::fs::read_to_string(Path::new(&path)) {
            Ok(s) => {
                let lines: Vec<&str> = s.lines().collect();
                let start = lines.len().saturating_sub(tail_lines.max(1));
                lines[start..].join("\n")
            }
            Err(_) => String::new(),
        }
    }

    // ===== Service-mode lifecycle notifiers (called by tools that own processes) =====

    pub fn notify_service_ready(&self, artifact_id: &str) {
        if let Some(a) = self.artifacts.lock().unwrap().get_mut(artifact_id) {
            if let Some(p) = a.process.as_mut() {
                p.status = "ready".to_string();
            }
        }
        self.event_bus
            .emit(EngineEvent::WorkbenchServiceReady(WorkbenchServiceReadyData {
                artifact_id: artifact_id.to_string(),
                ready: true,
            }));
    }

    pub fn notify_service_crashed(&self, artifact_id: &str, last_log_lines: String) {
        if let Some(a) = self.artifacts.lock().unwrap().get_mut(artifact_id) {
            if let Some(p) = a.process.as_mut() {
                p.status = "crashed".to_string();
            }
        }
        self.event_bus.emit(EngineEvent::WorkbenchServiceCrashed(
            WorkbenchServiceCrashedData {
                artifact_id: artifact_id.to_string(),
                last_log_lines,
            },
        ));
    }

    pub fn notify_service_stopped(&self, artifact_id: &str, reason: StopReason) {
        if let Some(a) = self.artifacts.lock().unwrap().get_mut(artifact_id) {
            if let Some(p) = a.process.as_mut() {
                p.status = "stopped".to_string();
            }
        }
        self.event_bus.emit(EngineEvent::WorkbenchServiceStopped(
            WorkbenchServiceStoppedData {
                artifact_id: artifact_id.to_string(),
                reason,
            },
        ));
    }

    /// Stop all live services on session end. Idempotent.
    pub fn shutdown(&self) {
        let ids: Vec<String> = self
            .artifacts
            .lock()
            .unwrap()
            .values()
            .filter(|a| a.process.is_some())
            .map(|a| a.id.clone())
            .collect();
        for id in ids {
            self.notify_service_stopped(&id, StopReason::SessionEnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_static(id: &str) -> WorkbenchArtifact {
        WorkbenchArtifact {
            id: id.to_string(),
            title: format!("Artifact {id}"),
            mode: WorkbenchMode::Static,
            files: vec![WorkbenchFile {
                path: "index.html".to_string(),
                content: Some("<h1>hi</h1>".to_string()),
                mime_type: Some("text/html".to_string()),
            }],
            url: None,
            process: None,
            usage: None,
            root_dir: None,
            created_at: 0,
        }
    }

    #[test]
    fn publish_and_list() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let svc = WorkbenchService::new(bus);
        svc.publish(mk_static("a"), None);
        assert_eq!(svc.list().len(), 1);
        assert!(matches!(rx.try_recv().unwrap(), EngineEvent::WorkbenchNew(_)));
    }

    #[test]
    fn replaces_id_removes_old() {
        let svc = WorkbenchService::new(EventBus::new());
        svc.publish(mk_static("a"), None);
        svc.publish(mk_static("b"), Some("a".to_string()));
        assert_eq!(svc.list().len(), 1);
        assert!(svc.get("a").is_none());
        assert!(svc.get("b").is_some());
    }

    #[test]
    fn mark_viewed_only_when_exists() {
        let svc = WorkbenchService::new(EventBus::new());
        assert!(!svc.mark_viewed("missing"));
        svc.publish(mk_static("x"), None);
        assert!(svc.mark_viewed("x"));
    }

    #[test]
    fn read_file_returns_inline_content() {
        let svc = WorkbenchService::new(EventBus::new());
        svc.publish(mk_static("c"), None);
        assert_eq!(svc.read_file("c", "index.html").unwrap(), "<h1>hi</h1>");
    }

    #[test]
    fn read_file_unknown_artifact() {
        let svc = WorkbenchService::new(EventBus::new());
        assert_eq!(svc.read_file("nope", "f").unwrap_err(), "not_found");
    }

    #[test]
    fn close_removes_artifact() {
        let svc = WorkbenchService::new(EventBus::new());
        svc.publish(mk_static("d"), None);
        assert!(svc.close("d"));
        assert!(!svc.close("d"));
        assert!(svc.get("d").is_none());
    }

    #[test]
    fn shutdown_only_stops_service_artifacts() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let svc = WorkbenchService::new(bus);
        svc.publish(mk_static("static"), None);
        let svc_artifact = WorkbenchArtifact {
            process: Some(ProcessInfo {
                status: "ready".to_string(),
                log_path: None,
            }),
            mode: WorkbenchMode::Web,
            ..mk_static("svc")
        };
        svc.publish(svc_artifact, None);
        // drain publish events
        while let Ok(ev) = rx.try_recv() {
            if matches!(ev, EngineEvent::WorkbenchServiceStopped(_)) {
                panic!("unexpected stop before shutdown");
            }
        }
        svc.shutdown();
        // exactly one stop event (svc), not for static
        let mut stops = 0;
        while let Ok(ev) = rx.try_recv() {
            if matches!(ev, EngineEvent::WorkbenchServiceStopped(_)) {
                stops += 1;
            }
        }
        assert_eq!(stops, 1);
    }
}
