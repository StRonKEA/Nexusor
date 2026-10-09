//! Integrates local application settings.
use std::{collections::BTreeMap, fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

const NO_PROXY_KEY: &str = "http.noProxy";
const KEYS: [&str; 7] = [
    "http.proxy",
    "http.proxyKerberosServicePrincipal",
    "http.proxySupport",
    "http.proxyStrictSSL",
    "cursor.general.disableHttp2",
    "http.experimental.systemCertificatesV2",
    "cursor.debug.timeoutPrevention",
];

fn path() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| Error::Config("cannot resolve user home directory".into()))?;
    Ok(std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData/Roaming"))
        .join("Cursor/User/settings.json"))
}

#[cfg(test)]
fn backup_path() -> Result<PathBuf> {
    let p = path()?;
    Ok(p.with_file_name("settings.json.nexusor.bak"))
}

fn read() -> Result<BTreeMap<String, Value>> {
    read_at(&path()?)
}

fn read_at(path: &std::path::Path) -> Result<BTreeMap<String, Value>> {
    let data = match fs::read_to_string(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error.into()),
    };
    if data.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    json5::from_str(&data)
        .map_err(|error| Error::Config(format!("parse Cursor settings JSONC: {error}")))
}

fn write_at(path: &std::path::Path, settings: &BTreeMap<String, Value>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_vec_pretty(settings)?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, [data.as_slice(), b"\n"].concat())?;
    fs::rename(temp, path)?;
    Ok(())
}

pub fn write_proxy_settings(proxy_url: &str) -> Result<()> {
    apply_at(&path()?, proxy_url)
}

fn managed_values(proxy_url: &str) -> BTreeMap<String, Value> {
    let mut settings = BTreeMap::new();
    settings.insert(
        NO_PROXY_KEY.into(),
        serde_json::json!(["localhost", "127.0.0.1", "::1"]),
    );
    settings.insert(KEYS[0].into(), Value::String(proxy_url.into()));
    settings.insert(KEYS[1].into(), Value::String(proxy_url.into()));
    settings.insert(KEYS[2].into(), Value::String("on".into()));
    settings.insert(KEYS[3].into(), Value::Bool(false));
    settings.insert(KEYS[4].into(), Value::Bool(true));
    settings.insert(KEYS[5].into(), Value::Bool(true));
    settings.insert(KEYS[6].into(), Value::String("always".into()));
    settings
}

pub fn clear_proxy_settings() -> Result<()> {
    restore_at(&path()?)
}

pub fn settings_match(proxy_url: &str) -> Result<bool> {
    let settings = read()?;
    Ok(
        settings.get(KEYS[0]) == Some(&Value::String(proxy_url.into()))
            && settings.get(NO_PROXY_KEY)
                == Some(&serde_json::json!(["localhost", "127.0.0.1", "::1"]))
            && settings.get(KEYS[1]) == Some(&Value::String(proxy_url.into()))
            && settings.get(KEYS[2]) == Some(&Value::String("on".into()))
            && settings.get(KEYS[3]) == Some(&Value::Bool(false))
            && settings.get(KEYS[4]) == Some(&Value::Bool(true))
            && settings.get(KEYS[5]) == Some(&Value::Bool(true))
            && settings.get(KEYS[6]) == Some(&Value::String("always".into())),
    )
}

pub fn clear_stale_managed_settings() -> Result<()> {
    clear_proxy_settings()
}

fn looks_managed(settings: &BTreeMap<String, Value>) -> bool {
    let managed_signature = settings.get(KEYS[2]) == Some(&Value::String("on".into()))
        && settings.get(KEYS[3]) == Some(&Value::Bool(false))
        && settings.get(KEYS[4]) == Some(&Value::Bool(true))
        && settings.get(KEYS[5]) == Some(&Value::Bool(true));
    let loopback = settings
        .get(KEYS[0])
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<reqwest::Url>().ok())
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1"));
    managed_signature && loopback
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ManagedSettings {
    previous: BTreeMap<String, RecordedValue>,
    applied: BTreeMap<String, Value>,
}

/// What a managed key held before Nexusor touched it.
///
/// A bare `Option<Value>` cannot express this: `None` and `Value::Null` both
/// serialize to JSON `null`, so a journal written from one would read back as the
/// other and disable would delete a key the user had explicitly set to null.
/// `Recorded` carries the third state, so all three round-trip exactly.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum RecordedValue {
    /// The shape written now.
    Recorded {
        present: bool,
        #[serde(default)]
        value: Option<Value>,
    },
    /// A record written before the three states could be told apart. A stored `null`
    /// meant either "absent" or "null", and cannot be recovered, so it is read as
    /// absent: that is the only choice that cannot leave a stale value behind.
    Legacy(Value),
}

impl RecordedValue {
    /// The record for whatever the file held, distinguishing absent from null.
    fn of(previous: Option<&Value>) -> Self {
        match previous {
            None => Self::Recorded {
                present: false,
                value: None,
            },
            Some(value) => Self::Recorded {
                present: true,
                value: Some(value.clone()),
            },
        }
    }

    /// The value to write back: `None` means the key must be removed.
    fn restore(&self) -> Option<Value> {
        match self {
            Self::Recorded { present: false, .. } => None,
            Self::Recorded {
                present: true,
                value,
            } => Some(value.clone().unwrap_or(Value::Null)),
            Self::Legacy(Value::Null) => None,
            Self::Legacy(value) => Some(value.clone()),
        }
    }
}

fn journal_path(path: &std::path::Path) -> PathBuf {
    path.with_file_name("settings.json.nexusor-managed.json")
}

fn apply_at(path: &std::path::Path, proxy_url: &str) -> Result<()> {
    let mut settings = read_at(path)?;
    let backup = path.with_file_name("settings.json.nexusor.bak");
    let journal = journal_path(path);
    let applied = managed_values(proxy_url);
    let mut record = if journal.exists() {
        let mut record: ManagedSettings = serde_json::from_slice(&fs::read(&journal)?)?;
        for key in KEYS.into_iter().chain([NO_PROXY_KEY]) {
            if !record.applied.contains_key(key) || !record.previous.contains_key(key) {
                return Err(Error::Config(
                    "Incomplete Nexusor settings restoration record".into(),
                ));
            }
        }
        for (key, old_value) in &record.applied {
            if settings.get(key) != Some(old_value) {
                record
                    .previous
                    .insert(key.clone(), RecordedValue::of(settings.get(key)));
            }
        }
        record
    } else {
        let original = if backup.exists() && looks_managed(&settings) {
            read_at(&backup)?
        } else {
            settings.clone()
        };
        ManagedSettings {
            previous: applied
                .keys()
                .map(|key| (key.clone(), RecordedValue::of(original.get(key))))
                .collect(),
            applied: BTreeMap::new(),
        }
    };
    record.applied = applied.clone();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path.exists() && !backup.exists() {
        fs::copy(path, &backup)?;
    }
    // Record restoration values before changing any Cursor settings.
    let temp = journal.with_extension("tmp");
    fs::write(&temp, serde_json::to_vec_pretty(&record)?)?;
    fs::rename(temp, &journal)?;
    settings.extend(applied);
    write_at(path, &settings)
}

fn restore_at(path: &std::path::Path) -> Result<()> {
    let mut settings = read_at(path)?;
    let journal = journal_path(path);
    let backup = path.with_file_name("settings.json.nexusor.bak");
    let record: ManagedSettings = if journal.exists() {
        serde_json::from_slice(&fs::read(&journal)?)?
    } else if looks_managed(&settings) {
        let original = read_at(&backup)?;
        let applied = managed_values(
            settings
                .get(KEYS[0])
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        ManagedSettings {
            previous: applied
                .keys()
                .map(|key| (key.clone(), RecordedValue::of(original.get(key))))
                .collect(),
            applied,
        }
    } else {
        return Ok(());
    };
    let before = settings.clone();
    // Later user edits, including deletions, take precedence over our baseline.
    for key in KEYS.into_iter().chain([NO_PROXY_KEY]) {
        let applied = record.applied.get(key).ok_or_else(|| {
            Error::Config("Incomplete Nexusor settings restoration record".into())
        })?;
        let previous = record.previous.get(key).ok_or_else(|| {
            Error::Config("Incomplete Nexusor settings restoration record".into())
        })?;
        if settings.get(key) != Some(applied) {
            continue;
        }
        match previous.restore() {
            Some(value) => {
                settings.insert(key.into(), value);
            }
            None => {
                settings.remove(key);
            }
        }
    }
    if settings != before {
        write_at(path, &settings)?;
    }
    if journal.exists() {
        fs::remove_file(journal)?;
    }
    if backup.exists() {
        fs::remove_file(backup)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disable_restores_original_values_and_preserves_later_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let original = BTreeMap::from([
            (
                "http.proxy".into(),
                serde_json::json!("http://old-proxy:8080"),
            ),
            ("http.noProxy".into(), serde_json::json!(["example.org"])),
            ("editor.fontSize".into(), serde_json::json!(14)),
        ]);
        write_at(&path, &original).unwrap();
        apply_at(&path, "http://127.0.0.1:6332").unwrap();
        apply_at(&path, "http://127.0.0.1:6333").unwrap();
        let mut edited = read_at(&path).unwrap();
        edited.insert("editor.fontSize".into(), serde_json::json!(18));
        edited.insert(KEYS[6].into(), serde_json::json!("never"));
        edited.remove(KEYS[5]);
        write_at(&path, &edited).unwrap();
        restore_at(&path).unwrap();
        let mut expected = original;
        expected.insert("editor.fontSize".into(), serde_json::json!(18));
        expected.insert(KEYS[6].into(), serde_json::json!("never"));
        assert_eq!(read_at(&path).unwrap(), expected);
        assert!(!journal_path(&path).exists());
        restore_at(&path).unwrap();
        assert_eq!(read_at(&path).unwrap(), expected);
    }

    #[test]
    fn fresh_profile_and_legacy_backup_restore_without_whole_file_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        apply_at(&path, "http://127.0.0.1:6332").unwrap();
        restore_at(&path).unwrap();
        assert!(read_at(&path).unwrap().is_empty());
        let backup = path.with_file_name("settings.json.nexusor.bak");
        write_at(
            &backup,
            &BTreeMap::from([
                ("http.proxy".into(), serde_json::json!("http://old:8080")),
                ("editor.fontSize".into(), serde_json::json!(10)),
            ]),
        )
        .unwrap();
        let mut settings = managed_values("http://127.0.0.1:6332");
        settings.insert("editor.fontSize".into(), serde_json::json!(20));
        write_at(&path, &settings).unwrap();
        restore_at(&path).unwrap();
        assert_eq!(
            read_at(&path).unwrap(),
            BTreeMap::from([
                ("http.proxy".into(), serde_json::json!("http://old:8080")),
                ("editor.fontSize".into(), serde_json::json!(20))
            ])
        );
    }

    #[test]
    fn corrupt_journal_fails_without_mutating_settings_or_losing_recovery_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        apply_at(&path, "http://127.0.0.1:6332").unwrap();
        let before = fs::read(&path).unwrap();
        fs::write(journal_path(&path), b"broken").unwrap();
        assert!(restore_at(&path).is_err());
        assert!(apply_at(&path, "http://127.0.0.1:6334").is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(journal_path(&path).exists());
    }

    #[test]
    fn foreign_proxy_with_stale_backup_is_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(
            &path,
            br#"{"http.proxy":"http://new-user-proxy:8080","editor.fontSize":18}"#,
        )
        .unwrap();
        fs::write(path.with_file_name("settings.json.nexusor.bak"), b"{}").unwrap();
        let before = fs::read(&path).unwrap();
        restore_at(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn reapply_preserves_user_override_as_next_baseline_and_incomplete_journal_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        apply_at(&path, "http://127.0.0.1:6332").unwrap();
        let mut settings = read_at(&path).unwrap();
        settings.insert(KEYS[0].into(), serde_json::json!("http://user:8080"));
        write_at(&path, &settings).unwrap();
        apply_at(&path, "http://127.0.0.1:6333").unwrap();
        restore_at(&path).unwrap();
        assert_eq!(
            read_at(&path).unwrap().get(KEYS[0]),
            Some(&serde_json::json!("http://user:8080"))
        );
        let before = fs::read(&path).unwrap();
        fs::write(journal_path(&path), br#"{"previous":{},"applied":{}}"#).unwrap();
        assert!(apply_at(&path, "http://127.0.0.1:6334").is_err());
        assert!(restore_at(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    /// A journal written before the three-state record existed must still be readable,
    /// or an existing install would start failing on the next apply or restore.
    #[test]
    fn a_journal_from_the_older_format_is_still_restorable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        // `http.proxy` was "user-choice", the rest were absent (stored as null).
        let legacy = serde_json::json!({
            "previous": {
                "http.proxy": "user-choice",
                "http.proxyKerberosServicePrincipal": Value::Null,
                "http.proxySupport": Value::Null,
                "http.proxyStrictSSL": Value::Null,
                "cursor.general.disableHttp2": Value::Null,
                "http.experimental.systemCertificatesV2": Value::Null,
                "cursor.debug.timeoutPrevention": Value::Null,
                "http.noProxy": Value::Null
            },
            "applied": managed_values("http://127.0.0.1:6332")
        });
        std::fs::write(
            journal_path(&path),
            serde_json::to_vec_pretty(&legacy).unwrap(),
        )
        .unwrap();
        write_at(
            &path,
            &managed_values("http://127.0.0.1:6332")
                .into_iter()
                .filter(|(key, _)| key == "http.proxy")
                .collect(),
        )
        .unwrap();
        restore_at(&path).unwrap();
        let after = read_at(&path).unwrap();
        assert_eq!(
            after.get("http.proxy"),
            Some(&serde_json::json!("user-choice"))
        );
        // Everything the old record could not tell apart is dropped, never guessed.
        assert!(!after.contains_key("http.proxySupport"));
        assert!(!journal_path(&path).exists());
    }

    #[test]
    fn backup_path_has_correct_extension() {
        let bak = backup_path().unwrap();
        assert!(bak.to_string_lossy().ends_with("settings.json.nexusor.bak"));
    }

    /// Rewriting the settings file must not drop entries the user left as an explicit
    /// `null`. A managed key that happens to be null has to round-trip, otherwise
    /// enabling Nexusor would quietly delete it from the user's settings.
    #[test]
    fn an_explicit_null_entry_survives_a_read_write_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let original = BTreeMap::from([
            ("editor.fontSize".into(), serde_json::json!(14)),
            (KEYS[0].into(), Value::Null),
            ("workbench.colorTheme".into(), Value::Null),
        ]);
        write_at(&path, &original).unwrap();
        assert_eq!(
            read_at(&path).unwrap(),
            original,
            "read dropped an explicit null entry"
        );
        // And the same must hold for a settings file that arrived as JSONC text.
        std::fs::write(&path, "{\n  // user comment\n  \"http.proxy\": null,\n}\n").unwrap();
        assert_eq!(
            read_at(&path).unwrap().get(KEYS[0]),
            Some(&Value::Null),
            "JSONC read dropped an explicit null entry"
        );
    }

    /// Every key Nexusor writes must be one it also takes back. A key added to the
    /// applied set without a matching restore entry would otherwise leave the user's
    /// Cursor settings changed forever, with no test failing.
    #[test]
    fn every_applied_key_is_named_in_the_restore_set() {
        let applied = managed_values("http://127.0.0.1:6332");
        let restore_set: std::collections::BTreeSet<&str> = KEYS
            .iter()
            .copied()
            .chain(std::iter::once(NO_PROXY_KEY))
            .collect();
        for key in applied.keys() {
            assert!(
                restore_set.contains(key.as_str()),
                "{key} is written by apply but not covered by restore"
            );
        }
        assert_eq!(
            applied.len(),
            restore_set.len(),
            "the applied set and the restore set have drifted apart"
        );
    }

    /// For every starting shape a managed key can be in, disabling must put the file
    /// back exactly as it was: absent keys stay absent, null and user values return.
    #[test]
    fn disable_restores_every_managed_key_from_every_starting_shape() {
        let shapes: [(&str, Option<Value>); 4] = [
            ("absent", None),
            ("null", Some(Value::Null)),
            ("user value", Some(serde_json::json!("user-choice"))),
            ("matching ours", Some(Value::Bool(true))),
        ];
        for (label, start) in shapes {
            for key in KEYS.into_iter().chain([NO_PROXY_KEY]) {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("settings.json");
                let mut before =
                    BTreeMap::from([("editor.fontSize".into(), serde_json::json!(14))]);
                if let Some(value) = start.clone() {
                    before.insert(key.into(), value.clone());
                }
                write_at(&path, &before).unwrap();

                apply_at(&path, "http://127.0.0.1:6332").unwrap();
                assert_eq!(
                    managed_values("http://127.0.0.1:6332").contains_key(key),
                    read_at(&path).unwrap().contains_key(key),
                    "{label} / {key}: apply did not write the key"
                );
                restore_at(&path).unwrap();
                assert_eq!(
                    read_at(&path).unwrap(),
                    before,
                    "{label} / {key}: disable did not restore the exact original"
                );
                assert!(
                    !journal_path(&path).exists(),
                    "{label} / {key}: journal left behind"
                );
                assert!(
                    !path.with_file_name("settings.json.nexusor.bak").exists(),
                    "{label} / {key}: backup left behind"
                );
            }
        }
    }

    /// A key the user removed after we wrote it must stay removed: our value is gone,
    /// so there is nothing of ours to undo.
    #[test]
    fn a_key_the_user_deleted_is_not_recreated_by_disable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        write_at(
            &path,
            &BTreeMap::from([("editor.fontSize".into(), serde_json::json!(14))]),
        )
        .unwrap();
        apply_at(&path, "http://127.0.0.1:6332").unwrap();
        let mut edited = read_at(&path).unwrap();
        for key in KEYS {
            edited.remove(key);
        }
        write_at(&path, &edited).unwrap();
        restore_at(&path).unwrap();
        let after = read_at(&path).unwrap();
        for key in KEYS {
            assert!(!after.contains_key(key), "{key} came back after disable");
        }
    }
}
