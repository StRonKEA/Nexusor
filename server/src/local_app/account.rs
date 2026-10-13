//! Integrates local Cursor account state.
//!
//! Enable writes [`managed_values`] into Cursor's `state.vscdb`; disable restores
//! the exact previous value of every key it wrote. A sidecar journal records the
//! baseline so the two directions stay symmetric: an account installed before the
//! journal existed still gets its keys removed, and a real Cursor session is never
//! written or overwritten.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::json;
use sqlx::{Connection, Row, SqliteConnection};

use crate::{Error, Result};

const EMAIL: &str = "cursor@ai.com";
const SIGN_UP_TYPE: &str = "Google";
const SUBJECT: &str = "cursor-local-user";
const MEMBERSHIP_TYPE: &str = "ultra";
const SUBSCRIPTION_STATUS: &str = "active";
const ACCESS_TOKEN_KEY: &str = "cursorAuth/accessToken";
const PRIVACY_MODE_KEY: &str = "cursorai/donotchange/privacyMode";
const FEATURE_FLAGS_KEY: &str = "workbench.experiments.featureFlagOverrides";

/// Feature-gate overrides, written as a user preference rather than account data.
///
/// `workbench.experiments.featureFlagOverrides` is Cursor's own documented local
/// override channel, and it is a workspace-level experiment preference - not part
/// of `cursorAuth`. It is therefore applied even when a real Cursor session is
/// present, which is what makes the gates below actually take effect for a user
/// who is signed in. Disable restores the previous value through the same journal
/// as every other managed key.
fn feature_flag_overrides() -> String {
    // Glass `ExperimentService._loadOverridesFromStorage` discards bare booleans
    // (`has no TTL, discarding`) and only accepts `{value, expiresAt}` objects
    // (`expiresAt: null` never expires). Every flag below is proven live in
    // Cursor 3.24.12: present in the `dAe` defaults table AND read through a
    // gate reader in `workbench.glass.main.js` or a `cursor-*` extension.
    // Production builds additionally require dev eligibility before applying
    // any override; the Statsig bootstrap in `analytics.rs` stays the primary
    // path for normal sessions.
    json!({
        "explicit_subagent_models": {"value": true, "expiresAt": null},
        "subagent_support_interrupt": {"value": true, "expiresAt": null},
        "opt_devs_into_experimental_model_toggle": {"value": true, "expiresAt": null},
        "meta_mcp_tool": {"value": true, "expiresAt": null},
        "mcp_input_schema_json": {"value": true, "expiresAt": null},
        "local_computer_use": {"value": true, "expiresAt": null},
        "windows_computer_use_batch": {"value": true, "expiresAt": null},
        "instant_grep_indexing": {"value": true, "expiresAt": null},
        "instant_grep_user_search": {"value": true, "expiresAt": null},
        // Gates the turn-completion handler that reopens a turn while a
        // `CreateGoal` goal is active. Without it `CreateGoal`/`UpdateGoal` would
        // record a goal that nothing ever continues, which is the dead promise
        // this catalogue exists to avoid.
        "agent_goal_continuation": {"value": true, "expiresAt": null}
    })
    .to_string()
}

/// Every key enable writes, and therefore every key disable is able to restore.
fn managed_values() -> Result<BTreeMap<String, String>> {
    let token = local_token()?;
    let feature_overrides = feature_flag_overrides();
    Ok(BTreeMap::from([
        (ACCESS_TOKEN_KEY.to_string(), token.clone()),
        ("cursorAuth/refreshToken".to_string(), token),
        ("cursorAuth/cachedEmail".to_string(), EMAIL.to_string()),
        (
            "cursorAuth/cachedSignUpType".to_string(),
            SIGN_UP_TYPE.to_string(),
        ),
        (
            "cursorAuth/stripeMembershipAuthId".to_string(),
            SUBJECT.to_string(),
        ),
        (
            "cursorAuth/stripeMembershipType".to_string(),
            MEMBERSHIP_TYPE.to_string(),
        ),
        (
            "cursorAuth/stripeSubscriptionStatus".to_string(),
            SUBSCRIPTION_STATUS.to_string(),
        ),
        (PRIVACY_MODE_KEY.to_string(), "true".to_string()),
        (
            "cursorai/donotchange/newPrivacyMode2".to_string(),
            r#"{"privacyMode":"PRIVACY_MODE_NO_TRAINING"}"#.to_string(),
        ),
        (FEATURE_FLAGS_KEY.to_string(), feature_overrides),
    ]))
}

pub async fn inject_if_missing() -> Result<()> {
    inject_if_missing_at(&state_db_path()?).await
}

pub async fn remove_injected_account_if_present() -> Result<()> {
    remove_injected_account_at(&state_db_path()?).await
}

/// A non-empty access token that is not ours means a real Cursor session exists.
async fn has_foreign_session(connection: &mut SqliteConnection) -> Result<bool> {
    let token = local_token()?;
    Ok(read_key(connection, ACCESS_TOKEN_KEY)
        .await?
        .is_some_and(|value| !value.trim().is_empty() && value != token))
}

async fn read_key(connection: &mut SqliteConnection, key: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query("SELECT CAST(value AS TEXT) AS value FROM ItemTable WHERE key = ?")
            .bind(key)
            .fetch_optional(connection)
            .await?
            .and_then(|row| row.try_get::<String, _>("value").ok()),
    )
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ManagedAccount {
    previous: BTreeMap<String, Option<String>>,
    applied: BTreeMap<String, String>,
}

fn journal_path(path: &Path) -> PathBuf {
    path.with_file_name("state.vscdb.nexusor-managed.json")
}

fn write_journal(journal: &Path, record: &ManagedAccount) -> Result<()> {
    if let Some(parent) = journal.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = journal.with_extension("tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(record)?)?;
    std::fs::rename(temp, journal)?;
    Ok(())
}

async fn remove_injected_account_at(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(path);
    let mut connection = match SqliteConnection::connect_with(&options).await {
        Ok(conn) => conn,
        Err(e) => {
            tracing::warn!(%e, "could not open state.vscdb to clean up injected account");
            return Ok(());
        }
    };
    let journal = journal_path(path);
    let record: Option<ManagedAccount> = if journal.exists() {
        Some(serde_json::from_slice(&std::fs::read(&journal)?)?)
    } else {
        None
    };
    // Without a journal (accounts written before it existed) only values that still
    // match ours exactly are removed, and only when this is still our session.
    let (applied, previous) = match record {
        Some(record) => (record.applied, record.previous),
        None => {
            let managed = managed_values()?;
            let token = local_token()?;
            if !read_key(&mut connection, ACCESS_TOKEN_KEY)
                .await?
                .is_some_and(|value| value == token)
            {
                return Ok(());
            }
            (
                managed.clone(),
                managed.keys().map(|key| (key.clone(), None)).collect(),
            )
        }
    };
    // Only keys still holding our value are touched; later user edits win.
    let mut plan: Vec<(String, Option<String>)> = Vec::new();
    for (key, ours) in &applied {
        let Some(original) = previous.get(key) else {
            continue;
        };
        if read_key(&mut connection, key).await?.as_deref() != Some(ours.as_str()) {
            continue;
        }
        plan.push((key.clone(), original.clone()));
    }
    if !plan.is_empty() {
        let mut transaction = connection.begin().await?;
        for (key, original) in &plan {
            match original {
                Some(value) => {
                    sqlx::query("INSERT OR REPLACE INTO ItemTable(key, value) VALUES(?, ?)")
                        .bind(key)
                        .bind(value)
                        .execute(&mut *transaction)
                        .await?;
                }
                None => {
                    sqlx::query("DELETE FROM ItemTable WHERE key = ?")
                        .bind(key)
                        .execute(&mut *transaction)
                        .await?;
                }
            }
        }
        transaction.commit().await?;
        tracing::info!("removed injected local Cursor account on disable");
    }
    if journal.exists() {
        std::fs::remove_file(&journal)?;
    }
    Ok(())
}

fn state_db_path() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| Error::Config("cannot resolve user home directory".into()))?;
    Ok(std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData/Roaming"))
        .join("Cursor/User/globalStorage/state.vscdb"))
}

async fn inject_if_missing_at(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)",
    )
    .execute(&mut connection)
    .await?;

    // A real Cursor session means this is the user's own account, not ours to
    // rewrite: `cursorAuth/*` and the privacy keys stay untouched. The
    // feature-gate override is a workspace preference rather than account data,
    // so it is still applied - otherwise every gate below would silently never
    // reach a signed-in user.
    let mut applied = managed_values()?;
    if has_foreign_session(&mut connection).await? {
        applied.retain(|key, _| key == FEATURE_FLAGS_KEY);
    }
    if applied.is_empty() {
        return Ok(());
    }

    // Record restoration values before writing any Cursor state.
    let journal = journal_path(path);
    let mut record = if journal.exists() {
        let mut record: ManagedAccount = serde_json::from_slice(&std::fs::read(&journal)?)?;
        for key in applied.keys() {
            if !record.applied.contains_key(key) || !record.previous.contains_key(key) {
                return Err(Error::Config(
                    "Incomplete Nexusor account restoration record".into(),
                ));
            }
        }
        // Re-baseline any key that changed since we last wrote it.
        for key in applied.keys() {
            let current = read_key(&mut connection, key).await?;
            if current.as_deref() != record.applied.get(key).map(String::as_str) {
                record.previous.insert(key.clone(), current);
            }
        }
        record
    } else {
        let mut previous = BTreeMap::new();
        for key in applied.keys() {
            previous.insert(key.clone(), read_key(&mut connection, key).await?);
        }
        ManagedAccount {
            previous,
            applied: BTreeMap::new(),
        }
    };
    // Merge rather than replace. A run that only writes the gate overrides - for
    // example because a real Cursor session now owns `cursorAuth` - must not drop
    // the record of an account an earlier run injected, or disable would leave
    // those keys stuck at the injected values with nothing left to restore from.
    for (key, value) in &applied {
        record.applied.insert(key.clone(), value.clone());
    }
    write_journal(&journal, &record)?;

    let mut transaction = connection.begin().await?;
    for (key, value) in &applied {
        sqlx::query("INSERT OR REPLACE INTO ItemTable(key, value) VALUES(?, ?)")
            .bind(key)
            .bind(value)
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;
    if applied.len() == 1 {
        tracing::info!(
            key = FEATURE_FLAGS_KEY,
            "applied feature-gate overrides alongside an existing Cursor session"
        );
    } else {
        tracing::info!(
            email = EMAIL,
            subject = SUBJECT,
            "injected local Cursor account"
        );
    }
    Ok(())
}

pub(crate) fn is_local_cursor_authorization(authorization: &str) -> bool {
    authorization
        .strip_prefix("Bearer ")
        .is_some_and(is_local_cursor_token)
}

fn is_local_cursor_token(token: &str) -> bool {
    local_token().is_ok_and(|local| local == token)
}

pub(super) fn local_token() -> Result<String> {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({
        "sub": SUBJECT,
        "email": EMAIL,
        "type": "session",
        "iss": "cursor-client",
        "scope": "openid profile email",
        "exp": 4070908800_u64
    }))?);
    Ok(format!("{header}.{payload}.{SUBJECT}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn real_account_and_user_feature_overrides_survive_enable_and_disable() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.vscdb");
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        sqlx::query("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB)")
            .execute(&mut connection)
            .await
            .unwrap();
        for (key, value) in [
            ("cursorAuth/accessToken", "real-test-session"),
            ("cursorAuth/stripeMembershipType", "free"),
            (
                "workbench.experiments.featureFlagOverrides",
                r#"{"userFlag":true}"#,
            ),
        ] {
            sqlx::query("INSERT INTO ItemTable VALUES (?, ?)")
                .bind(key)
                .bind(value)
                .execute(&mut connection)
                .await
                .unwrap();
        }
        let before: Vec<(String, String)> =
            sqlx::query_as("SELECT key,CAST(value AS TEXT) FROM ItemTable ORDER BY key")
                .fetch_all(&mut connection)
                .await
                .unwrap();
        drop(connection);
        inject_if_missing_at(&path).await.unwrap();
        remove_injected_account_at(&path).await.unwrap();
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        let after: Vec<(String, String)> =
            sqlx::query_as("SELECT key,CAST(value AS TEXT) FROM ItemTable ORDER BY key")
                .fetch_all(&mut connection)
                .await
                .unwrap();
        assert_eq!(after, before);
    }

    /// Regression guard for the audit finding: with a real Cursor session the
    /// account keys must stay untouched, but the gate overrides are a workspace
    /// preference and must still be written, otherwise every gate in
    /// `feature_flag_overrides` silently never reaches a signed-in user.
    #[tokio::test]
    async fn gate_overrides_reach_a_real_session_while_account_keys_do_not() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.vscdb");
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        sqlx::query("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB)")
            .execute(&mut connection)
            .await
            .unwrap();
        for (key, value) in [
            ("cursorAuth/accessToken", "real-test-session"),
            ("cursorAuth/cachedEmail", "real@example.com"),
            ("cursorAuth/stripeMembershipType", "free"),
        ] {
            sqlx::query("INSERT INTO ItemTable VALUES (?, ?)")
                .bind(key)
                .bind(value)
                .execute(&mut connection)
                .await
                .unwrap();
        }
        drop(connection);

        inject_if_missing_at(&path).await.unwrap();

        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        for (key, expected) in [
            ("cursorAuth/accessToken", "real-test-session"),
            ("cursorAuth/cachedEmail", "real@example.com"),
            ("cursorAuth/stripeMembershipType", "free"),
        ] {
            let actual: Option<String> =
                sqlx::query_scalar("SELECT CAST(value AS TEXT) FROM ItemTable WHERE key = ?")
                    .bind(key)
                    .fetch_one(&mut connection)
                    .await
                    .unwrap();
            assert_eq!(actual.as_deref(), Some(expected), "{key} was rewritten");
        }
        let overrides: String =
            sqlx::query_scalar("SELECT CAST(value AS TEXT) FROM ItemTable WHERE key = ?")
                .bind(FEATURE_FLAGS_KEY)
                .fetch_one(&mut connection)
                .await
                .unwrap();
        for gate in [
            "explicit_subagent_models",
            "subagent_support_interrupt",
            "opt_devs_into_experimental_model_toggle",
            "meta_mcp_tool",
            "mcp_input_schema_json",
        ] {
            assert!(overrides.contains(gate), "{gate} missing from overrides");
        }
    }

    /// A run that writes only the gate overrides must not erase the restoration
    /// record of an account an earlier run injected. Losing it would strand those
    /// keys at the injected values forever, because disable restores only what
    /// the record still lists.
    #[tokio::test]
    async fn a_gate_only_run_keeps_an_earlier_account_record() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.vscdb");
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        sqlx::query("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB)")
            .execute(&mut connection)
            .await
            .unwrap();
        sqlx::query("INSERT INTO ItemTable VALUES (?, ?)")
            .bind(ACCESS_TOKEN_KEY)
            .bind("real-test-session")
            .execute(&mut connection)
            .await
            .unwrap();
        drop(connection);

        // An earlier run recorded an account injection with nothing set before it.
        let journal = journal_path(&path);
        let managed = managed_values().unwrap();
        let record = ManagedAccount {
            previous: managed.keys().map(|key| (key.clone(), None)).collect(),
            applied: managed.clone(),
        };
        write_journal(&journal, &record).unwrap();

        // This run sees a real session and writes only the gate overrides.
        inject_if_missing_at(&path).await.unwrap();

        let restored: ManagedAccount =
            serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
        for key in managed.keys() {
            assert!(
                restored.applied.contains_key(key),
                "{key} lost its restoration record"
            );
        }
    }

    #[tokio::test]
    async fn reinjection_repairs_local_membership_cache() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.vscdb");
        inject_if_missing_at(&path).await.unwrap();

        let mut connection = SqliteConnection::connect(&format!("sqlite:{}", path.display()))
            .await
            .unwrap();
        sqlx::query("UPDATE ItemTable SET value = 'free' WHERE key = ?")
            .bind("cursorAuth/stripeMembershipType")
            .execute(&mut connection)
            .await
            .unwrap();
        sqlx::query("DELETE FROM ItemTable WHERE key = ?")
            .bind("cursorAuth/stripeMembershipAuthId")
            .execute(&mut connection)
            .await
            .unwrap();
        drop(connection);

        inject_if_missing_at(&path).await.unwrap();

        let mut connection = SqliteConnection::connect(&format!("sqlite:{}", path.display()))
            .await
            .unwrap();
        let membership_type: String =
            sqlx::query_scalar("SELECT CAST(value AS TEXT) FROM ItemTable WHERE key = ?")
                .bind("cursorAuth/stripeMembershipType")
                .fetch_one(&mut connection)
                .await
                .unwrap();
        let membership_auth_id: String =
            sqlx::query_scalar("SELECT CAST(value AS TEXT) FROM ItemTable WHERE key = ?")
                .bind("cursorAuth/stripeMembershipAuthId")
                .fetch_one(&mut connection)
                .await
                .unwrap();

        assert_eq!(membership_type, MEMBERSHIP_TYPE);
        assert_eq!(membership_auth_id, SUBJECT);
    }

    #[tokio::test]
    async fn disable_removes_every_key_enable_wrote() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.vscdb");
        inject_if_missing_at(&path).await.unwrap();

        let mut connection = SqliteConnection::connect(&format!("sqlite:{}", path.display()))
            .await
            .unwrap();
        let injected: Vec<String> = sqlx::query_scalar("SELECT key FROM ItemTable ORDER BY key")
            .fetch_all(&mut connection)
            .await
            .unwrap();
        assert_eq!(
            injected.len(),
            managed_values().unwrap().len(),
            "enable must write exactly the managed key set"
        );
        drop(connection);

        remove_injected_account_at(&path).await.unwrap();

        let mut connection = SqliteConnection::connect(&format!("sqlite:{}", path.display()))
            .await
            .unwrap();
        let remaining: Vec<String> = sqlx::query_scalar("SELECT key FROM ItemTable ORDER BY key")
            .fetch_all(&mut connection)
            .await
            .unwrap();
        assert!(
            remaining.is_empty(),
            "disable left keys behind: {remaining:?}"
        );
        assert!(!journal_path(&path).exists());
    }

    /// The journal stores each previous value as text, so a key the user had set to
    /// the JSON value `null` must come back as `null` rather than being deleted. This
    /// is the same three-state case the Cursor settings record has to handle.
    #[tokio::test]
    async fn a_stored_json_null_value_is_restored_rather_than_deleted() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.vscdb");
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        sqlx::query("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB)")
            .execute(&mut connection)
            .await
            .unwrap();
        let key = PRIVACY_MODE_KEY;
        sqlx::query("INSERT INTO ItemTable VALUES(?, ?)")
            .bind(key)
            .bind("null")
            .execute(&mut connection)
            .await
            .unwrap();
        drop(connection);

        inject_if_missing_at(&path).await.unwrap();
        remove_injected_account_at(&path).await.unwrap();

        let mut connection = SqliteConnection::connect(&format!("sqlite:{}", path.display()))
            .await
            .unwrap();
        let restored: Option<String> =
            sqlx::query_scalar("SELECT CAST(value AS TEXT) FROM ItemTable WHERE key = ?")
                .bind(key)
                .fetch_optional(&mut connection)
                .await
                .unwrap();
        assert_eq!(
            restored.as_deref(),
            Some("null"),
            "the user's own null value was deleted instead of restored"
        );
    }

    #[tokio::test]
    async fn disable_restores_the_users_own_values_for_managed_keys() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.vscdb");
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        sqlx::query("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB)")
            .execute(&mut connection)
            .await
            .unwrap();
        for (key, value) in [
            (PRIVACY_MODE_KEY, "false"),
            (FEATURE_FLAGS_KEY, r#"{"userFlag":true}"#),
        ] {
            sqlx::query("INSERT INTO ItemTable VALUES (?, ?)")
                .bind(key)
                .bind(value)
                .execute(&mut connection)
                .await
                .unwrap();
        }
        drop(connection);

        inject_if_missing_at(&path).await.unwrap();
        remove_injected_account_at(&path).await.unwrap();

        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        assert_eq!(
            read_key(&mut connection, PRIVACY_MODE_KEY).await.unwrap(),
            Some("false".into())
        );
        assert_eq!(
            read_key(&mut connection, FEATURE_FLAGS_KEY).await.unwrap(),
            Some(r#"{"userFlag":true}"#.into())
        );
        assert_eq!(
            read_key(&mut connection, "cursorai/donotchange/newPrivacyMode2")
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn user_edits_after_enable_survive_disable() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.vscdb");
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        inject_if_missing_at(&path).await.unwrap();
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        sqlx::query("UPDATE ItemTable SET value = 'opt_out' WHERE key = ?")
            .bind(PRIVACY_MODE_KEY)
            .execute(&mut connection)
            .await
            .unwrap();
        sqlx::query("DELETE FROM ItemTable WHERE key = ?")
            .bind(FEATURE_FLAGS_KEY)
            .execute(&mut connection)
            .await
            .unwrap();
        drop(connection);

        remove_injected_account_at(&path).await.unwrap();

        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        assert_eq!(
            read_key(&mut connection, PRIVACY_MODE_KEY).await.unwrap(),
            Some("opt_out".into())
        );
        assert_eq!(
            read_key(&mut connection, FEATURE_FLAGS_KEY).await.unwrap(),
            None
        );
        // Untouched managed keys are still cleaned up.
        assert_eq!(
            read_key(&mut connection, ACCESS_TOKEN_KEY).await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn legacy_account_without_journal_is_still_cleaned_up() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.vscdb");
        inject_if_missing_at(&path).await.unwrap();
        // Simulate an install from before the journal existed.
        std::fs::remove_file(journal_path(&path)).unwrap();

        remove_injected_account_at(&path).await.unwrap();

        let mut connection = SqliteConnection::connect(&format!("sqlite:{}", path.display()))
            .await
            .unwrap();
        let remaining: Vec<String> = sqlx::query_scalar("SELECT key FROM ItemTable ORDER BY key")
            .fetch_all(&mut connection)
            .await
            .unwrap();
        assert!(
            remaining.is_empty(),
            "legacy cleanup left keys behind: {remaining:?}"
        );
    }

    #[tokio::test]
    async fn incomplete_restoration_record_fails_without_touching_cursor_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.vscdb");
        inject_if_missing_at(&path).await.unwrap();
        let mut connection = SqliteConnection::connect(&format!("sqlite:{}", path.display()))
            .await
            .unwrap();
        let before: Vec<(String, String)> =
            sqlx::query_as("SELECT key, CAST(value AS TEXT) FROM ItemTable ORDER BY key")
                .fetch_all(&mut connection)
                .await
                .unwrap();
        drop(connection);
        std::fs::write(journal_path(&path), br#"{"previous":{},"applied":{}}"#).unwrap();

        assert!(inject_if_missing_at(&path).await.is_err());
        let mut connection = SqliteConnection::connect(&format!("sqlite:{}", path.display()))
            .await
            .unwrap();
        let after: Vec<(String, String)> =
            sqlx::query_as("SELECT key, CAST(value AS TEXT) FROM ItemTable ORDER BY key")
                .fetch_all(&mut connection)
                .await
                .unwrap();
        assert_eq!(after, before);
        assert!(journal_path(&path).exists());
    }

    #[test]
    fn recognizes_only_the_injected_cursor_token() {
        let token = local_token().unwrap();
        assert!(is_local_cursor_token(&token));
        assert!(is_local_cursor_authorization(&format!("Bearer {token}")));
        assert!(!is_local_cursor_authorization(&token));
        assert!(!is_local_cursor_authorization(
            "Bearer official-cursor-token"
        ));
        assert!(!is_local_cursor_token("official-cursor-token"));
        assert!(!is_local_cursor_token(""));
    }

    /// Glass `ExperimentService._loadOverridesFromStorage` discards bare
    /// booleans (`has no TTL, discarding`); overrides must be
    /// `{value, expiresAt}` objects with `expiresAt: null` for no expiry.
    #[test]
    fn feature_flag_overrides_use_ttl_objects_the_client_accepts() {
        let managed = managed_values().unwrap();
        let raw = managed.get(FEATURE_FLAGS_KEY).unwrap();
        let overrides: serde_json::Value = serde_json::from_str(raw).unwrap();
        let overrides = overrides.as_object().unwrap();
        let names: Vec<&str> = overrides.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "agent_goal_continuation",
                "explicit_subagent_models",
                "instant_grep_indexing",
                "instant_grep_user_search",
                "local_computer_use",
                "mcp_input_schema_json",
                "meta_mcp_tool",
                "opt_devs_into_experimental_model_toggle",
                "subagent_support_interrupt",
                "windows_computer_use_batch",
            ]
            .to_vec(),
            "only gates proven live in 3.24.12 may be overridden"
        );
        for (name, entry) in overrides {
            let entry = entry.as_object().unwrap_or_else(|| {
                panic!("{name} must be a {{value, expiresAt}} object, not a boolean")
            });
            assert_eq!(entry.get("value"), Some(&serde_json::Value::Bool(true)));
            assert_eq!(entry.get("expiresAt"), Some(&serde_json::Value::Null));
        }
    }
}
