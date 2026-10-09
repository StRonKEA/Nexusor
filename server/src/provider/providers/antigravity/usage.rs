use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Result;

pub const ANTIGRAVITY_ENDPOINTS: &[&str] = &[
    "https://cloudcode-pa.googleapis.com",
    "https://daily-cloudcode-pa.googleapis.com",
    "https://daily-cloudcode-pa.sandbox.googleapis.com",
];

const USER_AGENT: &str =
    "Antigravity/4.3.0 (Macintosh; Intel Mac OS X 10_15_7) Chrome/132.0.6834.160 Electron/39.2.3";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelQuota {
    pub remaining_percent: f64,
    pub reset_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AntigravityQuota {
    pub plan_label: String,
    pub project_id: String,
    pub claude: Option<ModelQuota>,
    pub gemini: Option<ModelQuota>,
    #[serde(default)]
    pub claude_weekly: Option<ModelQuota>,
    #[serde(default)]
    pub gemini_weekly: Option<ModelQuota>,
}

pub async fn query_usage(client: &reqwest::Client, access_token: &str) -> Result<AntigravityQuota> {
    let (project_id, plan_label) = fetch_project_and_tier(client, access_token).await;

    // 1. Try modern retrieveUserQuotaSummary first (matches opencodex)
    for endpoint in ANTIGRAVITY_ENDPOINTS {
        let url = format!("{endpoint}/v1internal:retrieveUserQuotaSummary");
        let res = client
            .post(&url)
            .header("authorization", format!("Bearer {access_token}"))
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .header("user-agent", USER_AGENT)
            .header("x-client-name", "antigravity")
            .header("x-client-version", "4.3.0")
            .json(&serde_json::json!({ "project": project_id }))
            .send()
            .await;

        let Ok(response) = res else { continue };
        if !response.status().is_success() {
            continue;
        }

        let Ok(text) = response.text().await else {
            continue;
        };
        let Ok(json): std::result::Result<Value, _> = serde_json::from_str(&text) else {
            continue;
        };

        if let Some(parsed) = parse_user_quota_summary(&json, &project_id, &plan_label) {
            return Ok(parsed);
        }
    }

    // 2. Fallback to fetchAvailableModels with deep quota extraction
    for endpoint in ANTIGRAVITY_ENDPOINTS {
        let url = format!("{endpoint}/v1internal:fetchAvailableModels");
        let res = client
            .post(&url)
            .header("authorization", format!("Bearer {access_token}"))
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .header("user-agent", USER_AGENT)
            .header("x-client-name", "antigravity")
            .header("x-client-version", "4.3.0")
            .json(&serde_json::json!({ "project": project_id }))
            .send()
            .await;

        let Ok(response) = res else { continue };
        if !response.status().is_success() {
            continue;
        };

        let Ok(text) = response.text().await else {
            continue;
        };
        let Ok(json): std::result::Result<Value, _> = serde_json::from_str(&text) else {
            continue;
        };

        if let Some(parsed) = parse_available_models_quota(&json, &project_id, &plan_label) {
            return Ok(parsed);
        }
    }

    Err(crate::Error::Provider(
        "Antigravity quota could not be retrieved from any endpoint".into(),
    ))
}

fn parse_reset_time(val: Option<&Value>) -> Option<i64> {
    let s = val?.as_str()?;
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp_millis());
    }
    // Also try parse integer seconds/milliseconds directly
    if let Ok(num) = s.parse::<i64>() {
        return Some(if num < 100_000_000_000 {
            num * 1000
        } else {
            num
        });
    }
    None
}

fn extract_remaining_fraction(obj: &serde_json::Map<String, Value>) -> Option<f64> {
    if let Some(frac) = obj.get("remainingFraction").and_then(Value::as_f64) {
        return Some(frac.clamp(0.0, 1.0));
    }
    if let Some(pct) = obj.get("remainingPercentage").and_then(Value::as_f64) {
        return Some((pct / 100.0).clamp(0.0, 1.0));
    }
    if let Some(rem_obj) = obj.get("remaining").and_then(Value::as_object) {
        if let Some(frac) = rem_obj.get("remainingFraction").and_then(Value::as_f64) {
            return Some(frac.clamp(0.0, 1.0));
        }
        if let Some(pct) = rem_obj.get("remainingPercentage").and_then(Value::as_f64) {
            return Some((pct / 100.0).clamp(0.0, 1.0));
        }
    }
    None
}

fn parse_user_quota_summary(
    json: &Value,
    project_id: &str,
    plan_label: &str,
) -> Option<AntigravityQuota> {
    let groups = json.get("groups")?.as_array()?;
    if groups.is_empty() {
        return None;
    }

    let mut claude_5h: Option<ModelQuota> = None;
    let mut claude_weekly: Option<ModelQuota> = None;
    let mut gemini_5h: Option<ModelQuota> = None;
    let mut gemini_weekly: Option<ModelQuota> = None;

    for g in groups {
        let g_obj = g.as_object()?;
        let name_desc = format!(
            "{} {}",
            g_obj
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or(""),
            g_obj
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
        )
        .to_ascii_lowercase();

        let is_gemini = name_desc.contains("gemini");
        let is_claude =
            name_desc.contains("claude") || name_desc.contains("3p") || name_desc.contains("gpt");

        let buckets = g_obj.get("buckets").and_then(Value::as_array)?;
        for b in buckets {
            let b_obj = match b.as_object() {
                Some(o) => o,
                None => continue,
            };

            let frac = match extract_remaining_fraction(b_obj) {
                Some(f) => f,
                None => continue,
            };

            let reset_ms = parse_reset_time(b_obj.get("resetTime"));

            let window_desc = format!(
                "{} {} {}",
                b_obj.get("window").and_then(Value::as_str).unwrap_or(""),
                b_obj.get("bucketId").and_then(Value::as_str).unwrap_or(""),
                b_obj
                    .get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or("")
            )
            .to_ascii_lowercase();

            let is_weekly = window_desc.contains("week");

            let quota = ModelQuota {
                remaining_percent: (frac * 100.0).round(),
                reset_at_ms: reset_ms,
            };

            if is_gemini {
                if is_weekly {
                    gemini_weekly = Some(quota);
                } else if gemini_5h.is_none() {
                    gemini_5h = Some(quota);
                }
            } else if is_claude {
                if is_weekly {
                    claude_weekly = Some(quota);
                } else if claude_5h.is_none() {
                    claude_5h = Some(quota);
                }
            }
        }
    }

    if claude_5h.is_some()
        || gemini_5h.is_some()
        || claude_weekly.is_some()
        || gemini_weekly.is_some()
    {
        Some(AntigravityQuota {
            plan_label: plan_label.to_string(),
            project_id: project_id.to_string(),
            claude: claude_5h,
            gemini: gemini_5h,
            claude_weekly,
            gemini_weekly,
        })
    } else {
        None
    }
}

fn parse_available_models_quota(
    json: &Value,
    project_id: &str,
    plan_label: &str,
) -> Option<AntigravityQuota> {
    let models = json.get("models")?.as_object()?;

    let mut claude_quota: Option<ModelQuota> = None;
    let mut gemini_quota: Option<ModelQuota> = None;

    for (k, v) in models {
        let key = k.to_ascii_lowercase();
        let is_claude = key.contains("claude")
            || key.contains("sonnet")
            || key.contains("opus")
            || key.contains("haiku");
        let is_gemini = key.contains("gemini");

        if !is_claude && !is_gemini {
            continue;
        }

        // Collect all quota entries (quotaInfo, quotaInfos, quotaInfoByTier)
        let mut entries = Vec::new();

        if let Some(qi) = v.get("quotaInfo") {
            if let Some(arr) = qi.as_array() {
                entries.extend(arr.iter().filter_map(Value::as_object));
            } else if let Some(obj) = qi.as_object() {
                entries.push(obj);
            }
        }

        if let Some(qis) = v.get("quotaInfos").and_then(Value::as_array) {
            entries.extend(qis.iter().filter_map(Value::as_object));
        }

        if let Some(by_tier) = v.get("quotaInfoByTier").and_then(Value::as_object) {
            for (_, val) in by_tier {
                if let Some(arr) = val.as_array() {
                    entries.extend(arr.iter().filter_map(Value::as_object));
                } else if let Some(obj) = val.as_object() {
                    entries.push(obj);
                }
            }
        }

        for q in entries {
            if let Some(frac) = extract_remaining_fraction(q) {
                let reset_ms = parse_reset_time(q.get("resetTime"));
                let quota = ModelQuota {
                    remaining_percent: (frac * 100.0).round(),
                    reset_at_ms: reset_ms,
                };

                if is_claude
                    && claude_quota
                        .as_ref()
                        .is_none_or(|q| frac < q.remaining_percent / 100.0)
                {
                    claude_quota = Some(quota);
                } else if is_gemini
                    && gemini_quota
                        .as_ref()
                        .is_none_or(|q| frac < q.remaining_percent / 100.0)
                {
                    gemini_quota = Some(quota);
                }
            }
        }
    }

    if claude_quota.is_some() || gemini_quota.is_some() {
        Some(AntigravityQuota {
            plan_label: plan_label.to_string(),
            project_id: project_id.to_string(),
            claude: claude_quota,
            gemini: gemini_quota,
            claude_weekly: None,
            gemini_weekly: None,
        })
    } else {
        None
    }
}

async fn fetch_project_and_tier(client: &reqwest::Client, access_token: &str) -> (String, String) {
    for endpoint in ANTIGRAVITY_ENDPOINTS {
        let url = format!("{endpoint}/v1internal:loadCodeAssist");
        let res = client
            .post(&url)
            .header("authorization", format!("Bearer {access_token}"))
            .header("content-type", "application/json")
            .header("user-agent", USER_AGENT)
            .header("x-client-name", "antigravity")
            .header("x-client-version", "4.3.0")
            .json(&serde_json::json!({
                "metadata": { "ideType": "ANTIGRAVITY" }
            }))
            .send()
            .await;

        let Ok(response) = res else { continue };
        if !response.status().is_success() {
            continue;
        };

        let Ok(text) = response.text().await else {
            continue;
        };
        let Ok(body): std::result::Result<Value, _> = serde_json::from_str(&text) else {
            continue;
        };

        let project = body
            .get("cloudaicompanionProject")
            .and_then(Value::as_str)
            .map(String::from);

        let plan = map_code_assist_plan_label(&body);
        return (
            project.unwrap_or_else(|| "bamboo-precept-lgxtn".into()),
            plan,
        );
    }

    ("bamboo-precept-lgxtn".into(), "FREE".into())
}

/// Map loadCodeAssist subscription fields to a stable plan label.
/// Priority mirrors OmniRoute/Antigravity Manager: paidTier → currentTier → allowedTiers default.
fn map_code_assist_plan_label(body: &Value) -> String {
    if let Some(label) = tier_field_label(body.get("paidTier")) {
        return label;
    }

    let ineligible = body
        .get("ineligibleTiers")
        .and_then(Value::as_array)
        .is_some_and(|tiers| !tiers.is_empty());

    if !ineligible {
        if let Some(label) = tier_field_label(body.get("currentTier")) {
            return label;
        }
    } else if let Some(tiers) = body.get("allowedTiers").and_then(Value::as_array) {
        let default_tier = tiers
            .iter()
            .find(|tier| tier.get("isDefault").and_then(Value::as_bool) == Some(true))
            .or_else(|| tiers.first());
        if let Some(label) = tier_field_label(default_tier) {
            return label;
        }
    }

    if let Some(label) = tier_field_label(body.get("currentTier")) {
        return label;
    }

    "FREE".into()
}

fn tier_field_label(tier: Option<&Value>) -> Option<String> {
    let tier = tier?;
    let name = tier.get("name").and_then(Value::as_str);
    let id = tier.get("id").and_then(Value::as_str);
    name.or(id).map(normalize_plan_label)
}

fn normalize_plan_label(raw: &str) -> String {
    let upper = raw.to_ascii_uppercase();
    if upper.contains("ULTRA") {
        return "ULTRA".into();
    }
    if upper.contains("PRO")
        || upper.contains("PREMIUM")
        || upper.contains("GOOGLE_ONE")
        || upper.contains("GOOGLE ONE")
        || upper.contains("ONE_AI")
        || upper.contains("ONE AI")
    {
        return "PRO".into();
    }
    if upper.contains("ENTERPRISE") {
        return "ENTERPRISE".into();
    }
    if upper.contains("BUSINESS") || upper.contains("STANDARD") {
        return "BUSINESS".into();
    }
    if upper.contains("PLUS") {
        return "PLUS".into();
    }
    if upper.contains("LITE") || upper.contains("LIGHT") {
        return "LITE".into();
    }
    if upper.contains("FREE") || upper.contains("INDIVIDUAL") || upper.contains("LEGACY") {
        return "FREE".into();
    }
    raw.split(['-', '_'])
        .flat_map(str::split_whitespace)
        .next()
        .map(|part| part.to_ascii_uppercase())
        .unwrap_or_else(|| "FREE".into())
}
