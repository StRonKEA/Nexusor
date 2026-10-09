use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AntigravityModel {
    pub id: String,
    pub display_name: String,
    pub max_output_tokens: Option<u64>,
    pub images: bool,
    /// Empty: send `id` and map Cursor Effort to `thinkingLevel`.
    /// Otherwise the account has no bare id; Effort picks `id-<tier>`.
    pub effort_tiers: Vec<String>,
}

/// Compact catalog: effort tiers (Low/Medium/High) come from Cursor's model
/// picker `reasoning` parameter → Cloud Code `thinkingConfig.thinkingLevel`,
/// not separate model IDs (CLIProxyAPI + Gemini 3.x pattern).
#[allow(dead_code)]
pub fn fallback_models() -> Vec<AntigravityModel> {
    vec![
        AntigravityModel {
            id: "gemini-3.7-flash".into(),
            display_name: "Gemini 3.7 Flash".into(),
            max_output_tokens: Some(65536),
            images: true,
            effort_tiers: Vec::new(),
        },
        AntigravityModel {
            id: "gemini-3.6-flash".into(),
            display_name: "Gemini 3.6 Flash".into(),
            max_output_tokens: Some(65536),
            images: true,
            effort_tiers: Vec::new(),
        },
        AntigravityModel {
            id: "gemini-3.1-pro-preview".into(),
            display_name: "Gemini 3.1 Pro Preview".into(),
            max_output_tokens: Some(65536),
            images: true,
            effort_tiers: Vec::new(),
        },
        AntigravityModel {
            id: "gemini-2.5-pro".into(),
            display_name: "Gemini 2.5 Pro".into(),
            max_output_tokens: Some(65536),
            images: true,
            effort_tiers: Vec::new(),
        },
        AntigravityModel {
            id: "gemini-2.5-flash".into(),
            display_name: "Gemini 2.5 Flash".into(),
            max_output_tokens: Some(65536),
            images: true,
            effort_tiers: Vec::new(),
        },
        AntigravityModel {
            id: "gemini-2.5-flash-lite".into(),
            display_name: "Gemini 2.5 Flash Lite".into(),
            max_output_tokens: Some(65536),
            images: true,
            effort_tiers: Vec::new(),
        },
        AntigravityModel {
            id: "gemini-2.0-flash".into(),
            display_name: "Gemini 2.0 Flash".into(),
            max_output_tokens: Some(65536),
            images: true,
            effort_tiers: Vec::new(),
        },
        AntigravityModel {
            id: "gemini-2.0-flash-lite".into(),
            display_name: "Gemini 2.0 Flash Lite".into(),
            max_output_tokens: Some(65536),
            images: true,
            effort_tiers: Vec::new(),
        },
        AntigravityModel {
            id: "claude-sonnet-4-6".into(),
            display_name: "Claude Sonnet 4.6 (Antigravity)".into(),
            max_output_tokens: Some(64000),
            images: true,
            effort_tiers: Vec::new(),
        },
        AntigravityModel {
            id: "claude-opus-4-6".into(),
            display_name: "Claude Opus 4.6 (Antigravity)".into(),
            max_output_tokens: Some(64000),
            images: true,
            effort_tiers: Vec::new(),
        },
        AntigravityModel {
            id: "claude-3-7-sonnet".into(),
            display_name: "Claude 3.7 Sonnet (Antigravity)".into(),
            max_output_tokens: Some(64000),
            images: true,
            effort_tiers: Vec::new(),
        },
        AntigravityModel {
            id: "claude-3-5-sonnet".into(),
            display_name: "Claude 3.5 Sonnet (Antigravity)".into(),
            max_output_tokens: Some(64000),
            images: true,
            effort_tiers: Vec::new(),
        },
        AntigravityModel {
            id: "claude-3-5-haiku".into(),
            display_name: "Claude 3.5 Haiku (Antigravity)".into(),
            max_output_tokens: Some(64000),
            images: true,
            effort_tiers: Vec::new(),
        },
        AntigravityModel {
            id: "gpt-4o".into(),
            display_name: "GPT-4o (Antigravity)".into(),
            max_output_tokens: Some(65536),
            images: true,
            effort_tiers: Vec::new(),
        },
    ]
}

/// Cloud Code's internal stream endpoint 404s on a `models/<id>` path. The slug
/// from `fetchAvailableModels` is sent unchanged, including `-thinking` and
/// `-high` suffixes that are part of the real model name.
pub fn canonicalize_model_id(input: &str) -> String {
    input.trim().trim_start_matches("models/").to_owned()
}

/// Resolve the upstream slug and, for a bare Gemini id, the `thinkingLevel`.
///
/// Cursor's Effort control is the only thinking switch. When the account publishes
/// `model-low` / `model-high` instead of `model`, `effort_tiers` lists those suffixes
/// and Effort selects one of them. A bare id keeps its name and receives
/// `thinkingConfig.thinkingLevel` from the same Effort value.
pub fn resolve_model_and_thinking(
    model_id: &str,
    effort: Option<&str>,
    effort_tiers: &[String],
) -> (String, Option<String>) {
    let api_model = canonicalize_model_id(model_id);
    if effort_tiers.len() == 1 && effort_tiers[0] == "thinking" {
        return (format!("{api_model}-thinking"), None);
    }
    if !effort_tiers.is_empty() {
        let wanted = effort
            .and_then(normalize_thinking_level)
            .unwrap_or_else(|| "medium".into());
        let chosen = effort_tiers
            .iter()
            .find(|tier| tier.as_str() == wanted)
            .or_else(|| effort_tiers.iter().find(|tier| tier.as_str() == "medium"))
            .or_else(|| effort_tiers.iter().find(|tier| tier.as_str() == "low"))
            .or_else(|| effort_tiers.iter().find(|tier| tier.as_str() == "high"))
            .unwrap_or(&effort_tiers[0]);
        return (format!("{api_model}-{chosen}"), None);
    }
    let level = effort.and_then(normalize_thinking_level).filter(|lvl| {
        // gemini-2.5-flash / gemini-2.5-pro only support default thinking,
        // Google Cloud Code explicitly returns: "Thinking level LOW is not supported for this model"
        if api_model.contains("gemini-2.5") {
            lvl != "low" && lvl != "minimal"
        } else {
            api_model.contains("gemini")
        }
    });
    (api_model, level)
}

const EFFORT_SUFFIXES: &[&str] = &[
    "-extra-low",
    "-thinking",
    "-minimal",
    "-medium",
    "-tiered",
    "-high",
    "-low",
];

fn strip_effort_suffix(id: &str) -> &str {
    for suffix in EFFORT_SUFFIXES {
        if let Some(base) = id.strip_suffix(suffix) {
            if !base.is_empty() {
                return base;
            }
        }
    }
    id
}

fn effort_suffix(id: &str) -> Option<&str> {
    EFFORT_SUFFIXES.iter().find_map(|suffix| {
        id.strip_suffix(suffix)
            .filter(|base| !base.is_empty())
            .map(|_| suffix.trim_start_matches('-'))
    })
}

const AVAILABLE_MODELS_URL: &str =
    "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels";

/// Models the account can actually call. Internal routing ids (`chat_*`, `tab_*`)
/// are omitted; everything else is kept under the slug Cloud Code expects.
pub async fn fetch_models(
    client: &reqwest::Client,
    access_token: &str,
    project_id: &str,
) -> Result<Vec<AntigravityModel>, String> {
    let response = client
        .post(AVAILABLE_MODELS_URL)
        .bearer_auth(access_token)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header(
            "user-agent",
            super::provider::request_headers(access_token)["user-agent"].clone(),
        )
        .header("x-client-name", "antigravity")
        .header("x-client-version", "4.3.0")
        .json(&serde_json::json!({ "project": project_id }))
        .send()
        .await
        .map_err(|error| format!("Antigravity models request failed: {error}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| format!("Antigravity models body read failed: {error}"))?;
    if !status.is_success() {
        return Err(format!("Antigravity models HTTP {status}: {text}"));
    }
    let body: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("Antigravity models JSON parse failed: {error}"))?;
    let models = body
        .get("models")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "Antigravity models response has no models object".to_owned())?;
    let ids = models
        .keys()
        .filter(|id| !id.starts_with("chat_") && !id.starts_with("tab_"))
        .cloned()
        .collect::<Vec<_>>();
    let parsed = collapse_effort_variants(&ids);
    if parsed.is_empty() {
        return Err("Antigravity model list was empty".into());
    }
    Ok(parsed)
}

fn collapse_effort_variants(ids: &[String]) -> Vec<AntigravityModel> {
    let mut groups: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for id in ids {
        let tier = effort_suffix(id).unwrap_or("").to_owned();
        // `-tiered` is a routing bucket, not a Cursor Effort value.
        if tier == "tiered" {
            continue;
        }
        groups
            .entry(strip_effort_suffix(id).to_owned())
            .or_default()
            .push(tier);
    }
    groups
        .into_iter()
        .map(|(base, mut tiers)| {
            tiers.retain(|tier| !tier.is_empty());
            let bare_published = ids.iter().any(|id| id == &base);
            AntigravityModel {
                display_name: display_name(&base),
                images: base.contains("gemini") || base.contains("image"),
                max_output_tokens: Some(65_536),
                // A published bare id takes thinkingLevel from Cursor Effort.
                // Otherwise Effort selects one of the published suffixes.
                effort_tiers: if bare_published { Vec::new() } else { tiers },
                id: base,
            }
        })
        .collect()
}

fn display_name(id: &str) -> String {
    id.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) if first.is_ascii_alphabetic() => {
                    first.to_uppercase().collect::<String>() + chars.as_str()
                }
                _ => part.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Gemini 3.x thinkingLevel values: minimal | low | medium | high.
pub fn normalize_thinking_level(effort: &str) -> Option<String> {
    match effort.trim().to_ascii_lowercase().as_str() {
        "" | "none" | "off" => None,
        "minimal" | "min" => Some("minimal".into()),
        "low" => Some("low".into()),
        "medium" | "med" => Some("medium".into()),
        "high" | "xhigh" | "extra_high" | "extra-high" | "max" => Some("high".into()),
        other => Some(other.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_no_effort_tier_duplicates() {
        let models = fallback_models();
        assert!(models.iter().all(|m| {
            let id = m.id.as_str();
            !id.ends_with("-high") && !id.ends_with("-medium") && !id.ends_with("-low")
        }));
        assert!(models.iter().any(|m| m.id == "gemini-3.7-flash"));
        assert!(!models.iter().any(|m| m.id == "gemini-3.7-flash-high"));
    }

    #[test]
    fn effort_selects_published_tier_slug() {
        let tiers = vec!["low".into(), "high".into()];
        let (model, level) = resolve_model_and_thinking("gemini-3.6-flash", Some("low"), &tiers);
        assert_eq!(model, "gemini-3.6-flash-low");
        assert_eq!(level, None);
    }

    #[test]
    fn bare_gemini_uses_thinking_level_from_effort() {
        let (model, level) = resolve_model_and_thinking("gemini-2.5-flash", Some("high"), &[]);
        assert_eq!(model, "gemini-2.5-flash");
        assert_eq!(level.as_deref(), Some("high"));
    }

    #[test]
    fn thinking_only_family_keeps_thinking_slug() {
        let tiers = vec!["thinking".into()];
        let (model, level) = resolve_model_and_thinking("claude-opus-4-6", None, &tiers);
        assert_eq!(model, "claude-opus-4-6-thinking");
        assert_eq!(level, None);
    }

    #[test]
    fn xhigh_maps_to_gemini_high() {
        assert_eq!(normalize_thinking_level("xhigh").as_deref(), Some("high"));
        assert_eq!(normalize_thinking_level("max").as_deref(), Some("high"));
    }
}
