use std::collections::BTreeSet;

#[derive(Debug, Clone, serde::Serialize)]
pub struct PromptParseResult {
    pub command: Option<String>,
    pub refs: Vec<String>,
    pub skills: Vec<String>,
    pub plain_text: String,
    pub normalized_prompt: String,
}

pub fn parse_prompt(input: &str) -> PromptParseResult {
    let mut command: Option<String> = None;
    let mut refs = BTreeSet::new();
    let mut skills = BTreeSet::new();
    let mut plain_tokens: Vec<String> = Vec::new();

    for (idx, token) in input.split_whitespace().enumerate() {
        if idx == 0 && token.starts_with('/') && token.len() > 1 {
            command = Some(token.trim_start_matches('/').to_string());
            continue;
        }
        if token.starts_with('@') && token.len() > 1 {
            refs.insert(token.trim_start_matches('@').to_string());
            continue;
        }
        if token.starts_with('#') && token.len() > 1 {
            skills.insert(token.trim_start_matches('#').to_string());
            continue;
        }
        plain_tokens.push(token.to_string());
    }

    let refs_vec: Vec<String> = refs.into_iter().collect();
    let skills_vec: Vec<String> = skills.into_iter().collect();
    let plain_text = plain_tokens.join(" ").trim().to_string();

    let mut normalized = String::new();
    if let Some(cmd) = &command {
        normalized.push_str(&format!("[command:{cmd}]\n"));
    }
    if !refs_vec.is_empty() {
        normalized.push_str(&format!("[refs:{}]\n", refs_vec.join(", ")));
    }
    if !skills_vec.is_empty() {
        normalized.push_str(&format!("[skills:{}]\n", skills_vec.join(", ")));
    }
    normalized.push_str(&plain_text);

    PromptParseResult {
        command,
        refs: refs_vec,
        skills: skills_vec,
        plain_text,
        normalized_prompt: normalized.trim().to_string(),
    }
}
