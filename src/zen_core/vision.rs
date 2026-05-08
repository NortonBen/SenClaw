//! Vision capability resolution.
//!
//! Port of TS `util/vision.ts`.

use once_cell::sync::Lazy;
use regex::Regex;
use super::ModelProfile;

/// Vision patterns for model name matching.
static VISION_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        // OpenAI
        Regex::new(r"(?i)^gpt-4o").unwrap(),
        Regex::new(r"(?i)^gpt-4(\.\d+)?-vision").unwrap(),
        Regex::new(r"(?i)^gpt-5").unwrap(),
        Regex::new(r"(?i)^o1").unwrap(),
        Regex::new(r"(?i)^o3").unwrap(),
        Regex::new(r"(?i)^chatgpt-4o").unwrap(),
        // Anthropic Claude 3+ 全系支持视觉
        Regex::new(r"(?i)^claude-[34]").unwrap(),
        Regex::new(r"(?i)^claude-(opus|sonnet|haiku)-[34]").unwrap(),
        Regex::new(r"(?i)^anthropic/claude-3").unwrap(),
        // Qwen-VL 系列
        Regex::new(r"(?i)qwen.*-vl").unwrap(),
        Regex::new(r"(?i)qwen2(\.\d+)?-vl").unwrap(),
        Regex::new(r"(?i)qwen3(\.\d+)?-plus").unwrap(),
        Regex::new(r"(?i)qvq").unwrap(),
        // Moonshot Kimi vision
        Regex::new(r"(?i)moonshot-v1-.*-vision").unwrap(),
        Regex::new(r"(?i)kimi.*vision").unwrap(),
        Regex::new(r"(?i)^kimi-k2\.6").unwrap(),
        Regex::new(r"(?i)kimi-latest").unwrap(),
        // GLM-4V / Zhipu
        Regex::new(r"(?i)glm-4v").unwrap(),
        Regex::new(r"(?i)glm-4\.\d+v").unwrap(),
        // Google Gemini（经 OpenRouter 接入时）
        Regex::new(r"(?i)gemini.*pro").unwrap(),
        Regex::new(r"(?i)gemini.*flash").unwrap(),
        Regex::new(r"(?i)gemini-1\.5").unwrap(),
        Regex::new(r"(?i)gemini-2").unwrap(),
        // Llama 3.2 vision
        Regex::new(r"(?i)llama-3\.2.*vision").unwrap(),
        // 通用关键字
        Regex::new(r"(?i)-vl-").unwrap(),
        Regex::new(r"(?i)-vision").unwrap(),
        Regex::new(r"(?i)-vlm").unwrap(),
    ]
});

/// Infer vision capability from model name.
///
/// Returns true if the model name matches known vision patterns.
pub fn infer_vision(model_name: &str) -> bool {
    if model_name.is_empty() {
        return false;
    }
    VISION_PATTERNS.iter().any(|re| re.is_match(model_name))
}

/// Determine if a model has vision capability.
///
/// Explicit vision field takes priority; if not declared, infer from model name.
pub fn model_has_vision(profile: &ModelProfile) -> bool {
    if let Some(vision) = profile.vision {
        return vision;
    }
    infer_vision(&profile.model_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_vision_patterns() {
        assert!(infer_vision("gpt-4o"));
        assert!(infer_vision("gpt-4-vision"));
        assert!(infer_vision("claude-3-5-sonnet-20241022"));
        assert!(infer_vision("claude-opus-4-7"));
        assert!(infer_vision("qwen-vl-max"));
        assert!(infer_vision("qwen2.5-vl-72b-instruct"));
        assert!(infer_vision("moonshot-v1-8k-vision-preview"));
        assert!(infer_vision("glm-4v-plus"));
        assert!(infer_vision("deepseek-vl2"));
        assert!(!infer_vision("gpt-3.5-turbo"));
        assert!(!infer_vision("deepseek-chat"));
        assert!(!infer_vision("qwen-plus"));
    }

    #[test]
    fn test_model_has_vision_explicit_override() {
        let profile_with_vision = ModelProfile {
            name: "test".to_string(),
            provider: "test".to_string(),
            model_name: "gpt-3.5-turbo".to_string(),
            base_url: "http://test".to_string(),
            api_key: "test".to_string(),
            max_tokens: 1000,
            context_length: 4000,
            adapt: None,
            vision: Some(true),
        };
        assert!(model_has_vision(&profile_with_vision));

        let profile_without_vision = ModelProfile {
            name: "test".to_string(),
            provider: "test".to_string(),
            model_name: "gpt-4o".to_string(),
            base_url: "http://test".to_string(),
            api_key: "test".to_string(),
            max_tokens: 1000,
            context_length: 4000,
            adapt: None,
            vision: Some(false),
        };
        assert!(!model_has_vision(&profile_without_vision));
    }

    #[test]
    fn test_model_has_vision_inferred() {
        let profile = ModelProfile {
            name: "test".to_string(),
            provider: "test".to_string(),
            model_name: "gpt-4o".to_string(),
            base_url: "http://test".to_string(),
            api_key: "test".to_string(),
            max_tokens: 1000,
            context_length: 4000,
            adapt: None,
            vision: None,
        };
        assert!(model_has_vision(&profile));

        let profile_no_vision = ModelProfile {
            name: "test".to_string(),
            provider: "test".to_string(),
            model_name: "gpt-3.5-turbo".to_string(),
            base_url: "http://test".to_string(),
            api_key: "test".to_string(),
            max_tokens: 1000,
            context_length: 4000,
            adapt: None,
            vision: None,
        };
        assert!(!model_has_vision(&profile_no_vision));
    }
}