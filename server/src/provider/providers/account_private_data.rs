//! Normalizes plugin account private_data JSON to a single camelCase key set.
use serde_json::{Map, Value};

const KEY_PAIRS: &[(&str, &str)] = &[
    ("access_token", "accessToken"),
    ("refresh_token", "refreshToken"),
    ("account_id", "accountId"),
    ("project_id", "projectId"),
    ("display_name", "displayName"),
    ("expires_at_ms", "expiresAtMs"),
];

/// Prefer camelCase keys. If both snake_case and camelCase exist, keep camelCase.
pub fn normalize_account_private_data(value: Value) -> Value {
    let Value::Object(object) = value else {
        return value;
    };
    Value::Object(normalize_object(object))
}

fn normalize_object(mut object: Map<String, Value>) -> Map<String, Value> {
    for &(snake, camel) in KEY_PAIRS {
        if let Some(value) = object.remove(snake) {
            object.entry(camel.to_owned()).or_insert(value);
        }
    }
    object
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct SampleAccount {
        #[serde(rename = "accessToken")]
        access_token: String,
        #[serde(rename = "displayName", default)]
        display_name: String,
    }

    #[test]
    fn duplicate_display_name_keys_deserialize_after_normalize() {
        let raw = serde_json::json!({
            "accessToken": "tok",
            "display_name": "snake",
            "displayName": "camel",
        });
        let normalized = normalize_account_private_data(raw);
        let account: SampleAccount = serde_json::from_value(normalized).expect("deserialize");
        assert_eq!(account.access_token, "tok");
        assert_eq!(account.display_name, "camel");
    }

    #[test]
    fn snake_only_keys_are_promoted_to_camel_case() {
        let raw = serde_json::json!({
            "access_token": "tok",
            "display_name": "shown",
            "project_id": "proj",
        });
        let normalized = normalize_account_private_data(raw);
        let object = normalized.as_object().unwrap();
        assert_eq!(
            object.get("accessToken").and_then(Value::as_str),
            Some("tok")
        );
        assert_eq!(
            object.get("displayName").and_then(Value::as_str),
            Some("shown")
        );
        assert_eq!(
            object.get("projectId").and_then(Value::as_str),
            Some("proj")
        );
        assert!(!object.contains_key("access_token"));
        assert!(!object.contains_key("display_name"));
    }
}
