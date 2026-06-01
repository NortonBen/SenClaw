//! Checklist verification logic for dispatch tasks.

use super::types::{ChecklistItem, DispatchParent, DispatchTask, VerificationResult};
use std::collections::HashSet;

/// Verify a single task's checklist against its result and file changes.
pub fn verify_task_checklist(task: &DispatchTask) -> VerificationResult {
    if task.checklist.is_empty() {
        return VerificationResult {
            verified: true,
            missing_items: Vec::new(),
            failed_items: Vec::new(),
            warnings: vec!["No checklist defined for task".to_string()],
            note: Some("Task completed but has no checklist to verify".to_string()),
        };
    }

    let result_text = task.result.as_deref().unwrap_or("");
    let mut missing_items = Vec::new();
    let mut failed_items = Vec::new();
    let mut warnings = Vec::new();

    // Build dependency graph
    let item_map: std::collections::HashMap<&str, &ChecklistItem> = task
        .checklist
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect();

    // Check each checklist item
    for item in &task.checklist {
        // Check if dependencies are satisfied
        for dep_id in &item.depends_on {
            if let Some(dep_item) = item_map.get(dep_id.as_str()) {
                if dep_item.status != "completed" {
                    missing_items.push(format!(
                        "{}: dependency '{}' not completed",
                        item.description, dep_id
                    ));
                }
            }
        }

        // Check if item is marked as completed
        if item.status != "completed" {
            missing_items.push(format!("{}: not marked as completed", item.description));
            continue;
        }

        // Verify item against task result
        if !item.description.is_empty() {
            let description_lower = item.description.to_lowercase();
            if !result_text.to_lowercase().contains(&description_lower)
                && !verify_in_file_changes(&item.description, &task.file_changes)
            {
                failed_items.push(format!(
                    "{}: not found in task result or file changes",
                    item.description
                ));
            }
        }
    }

    // Check file changes against checklist
    if !task.file_changes.is_empty() {
        let file_paths: HashSet<&str> = task
            .file_changes
            .iter()
            .map(|fc| fc.path.as_str())
            .collect();
        for item in &task.checklist {
            if item.description.contains("file") || item.description.contains("create") {
                let has_file_change = file_paths.iter().any(|path| {
                    item.description
                        .to_lowercase()
                        .contains(&path.to_lowercase())
                        || path
                            .to_lowercase()
                            .contains(&item.description.to_lowercase())
                });
                if !has_file_change && item.status == "completed" {
                    warnings.push(format!(
                        "{}: marked completed but no matching file change found",
                        item.description
                    ));
                }
            }
        }
    }

    let verified = missing_items.is_empty() && failed_items.is_empty();

    VerificationResult {
        verified,
        missing_items,
        failed_items,
        warnings,
        note: if verified {
            Some("All checklist items verified successfully".to_string())
        } else {
            Some("Task verification failed - see missing/failed items".to_string())
        },
    }
}

/// Verify a parent dispatch's overall checklist completion.
pub fn verify_parent_checklist(parent: &DispatchParent) -> VerificationResult {
    let mut all_items = Vec::new();
    let mut completed_items = Vec::new();
    let mut failed_items = Vec::new();
    let mut warnings = Vec::new();

    // Collect all checklist items from all tasks
    for task in &parent.tasks {
        for item in &task.checklist {
            all_items.push(item.description.clone());
            if item.status == "completed" {
                completed_items.push(item.description.clone());
            } else if item.status == "failed" {
                failed_items.push(format!("{} (task: {})", item.description, task.label));
            }
        }

        // Check task verification results
        if let Some(ref verification) = task.verification_result {
            if !verification.verified {
                warnings.push(format!(
                    "Task '{}' verification failed: {}",
                    task.label,
                    verification.note.as_deref().unwrap_or("unknown reason")
                ));
            }
        }
    }

    let missing_items: Vec<String> = all_items
        .iter()
        .filter(|item| {
            !completed_items.contains(item)
                && !failed_items.iter().any(|f| f.contains(item.as_str()))
        })
        .cloned()
        .collect();

    let verified = missing_items.is_empty() && failed_items.is_empty();

    VerificationResult {
        verified,
        missing_items,
        failed_items,
        warnings,
        note: Some(format!(
            "Parent verification: {}/{} items completed",
            completed_items.len(),
            all_items.len()
        )),
    }
}

/// Helper to check if a checklist item is satisfied by file changes.
fn verify_in_file_changes(description: &str, file_changes: &[super::types::FileChange]) -> bool {
    let desc_lower = description.to_lowercase();
    for fc in file_changes {
        let path_lower = fc.path.to_lowercase();
        if desc_lower.contains(&path_lower) || path_lower.contains(&desc_lower) {
            return true;
        }
        if let Some(ref summary) = fc.summary {
            if summary.to_lowercase().contains(&desc_lower) {
                return true;
            }
        }
    }
    false
}

/// Auto-generate checklist items from a task prompt.
pub fn generate_checklist_from_prompt(prompt: &str) -> Vec<ChecklistItem> {
    let mut items = Vec::new();
    let lines: Vec<&str> = prompt.lines().collect();

    // Look for numbered lists or bullet points in the prompt
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Match numbered lists: "1.", "2.", etc.
        if let Some(rest) = trimmed.strip_prefix(|c: char| c.is_ascii_digit()) {
            if rest.starts_with('.') || rest.starts_with(')') {
                let description = rest[1..]
                    .trim()
                    .trim_start_matches(')')
                    .trim_start_matches('.')
                    .trim();
                if !description.is_empty() {
                    items.push(ChecklistItem {
                        id: format!("item-{}", i),
                        description: description.to_string(),
                        status: "pending".to_string(),
                        depends_on: Vec::new(),
                        verification_note: None,
                    });
                }
            }
        }

        // Match bullet points: "-", "*"
        if trimmed.starts_with('-') || trimmed.starts_with('*') {
            let description = trimmed[1..].trim();
            if !description.is_empty() {
                items.push(ChecklistItem {
                    id: format!("item-{}", i),
                    description: description.to_string(),
                    status: "pending".to_string(),
                    depends_on: Vec::new(),
                    verification_note: None,
                });
            }
        }
    }

    // If no structured list found, create a single item from the first sentence
    if items.is_empty() {
        if let Some(first_sentence) = prompt.split('.').next() {
            let description = first_sentence.trim();
            if !description.is_empty() {
                items.push(ChecklistItem {
                    id: "item-0".to_string(),
                    description: description.to_string(),
                    status: "pending".to_string(),
                    depends_on: Vec::new(),
                    verification_note: None,
                });
            }
        }
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_task_with_empty_checklist() {
        let task = DispatchTask {
            id: "test-1".to_string(),
            label: "Test Task".to_string(),
            agent_id: "agent-1".to_string(),
            agent_jid: "jid-1".to_string(),
            depends_on: Vec::new(),
            prompt: "Test prompt".to_string(),
            status: crate::agent::dispatch_bridge::types::DispatchTaskStatus::Done,
            result: Some("Task completed successfully".to_string()),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            started_at: None,
            timeout_seconds: 900,
            timeout_at: None,
            completed_at: None,
            is_virtual: false,
            persona_name: None,
            checklist: Vec::new(),
            file_changes: Vec::new(),
            verification_result: None,
        };

        let result = verify_task_checklist(&task);
        assert!(result.verified);
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn test_generate_checklist_from_prompt() {
        let prompt =
            "Implement feature X:\n1. Create new file\n2. Modify existing code\n3. Add tests";
        let checklist = generate_checklist_from_prompt(prompt);
        assert_eq!(checklist.len(), 3);
        assert_eq!(checklist[0].description, "Create new file");
        assert_eq!(checklist[1].description, "Modify existing code");
        assert_eq!(checklist[2].description, "Add tests");
    }
}
