//! Manages Cursor's normal BYOK preference, without altering account or program files.
use crate::{Error, Result};
use serde_json::{json, Value};
use sqlx::{Connection, SqliteConnection};
use std::path::{Path, PathBuf};

const USER: &str = "src.vs.platform.reactivestorage.browser.reactiveStorageServiceImpl.persistentStorage.applicationUser";
const BACKUP: &str = "nexusor.managed.useOpenAIKey.v1";

fn path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| Error::Config("cannot resolve user home".into()))?;
    Ok(std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData/Roaming"))
        .join("Cursor/User/globalStorage/state.vscdb"))
}

pub async fn enabled() -> Result<bool> {
    let path = path()?;
    if !path.exists() {
        return Ok(false);
    }
    let mut db = SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(path)
            .read_only(true),
    )
    .await?;
    let table: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='ItemTable'",
    )
    .fetch_one(&mut db)
    .await?;
    if table == 0 {
        return Ok(false);
    }
    let raw: Option<String> =
        sqlx::query_scalar("SELECT CAST(value AS TEXT) FROM ItemTable WHERE key=?")
            .bind(USER)
            .fetch_optional(&mut db)
            .await?;
    Ok(raw
        .map(|v| serde_json::from_str::<Value>(&v))
        .transpose()?
        .is_some_and(|v| v["useOpenAIKey"] == true))
}

pub async fn apply() -> Result<()> {
    update(&path()?, true).await
}
pub async fn restore() -> Result<()> {
    update(&path()?, false).await
}

async fn update(path: &Path, enable: bool) -> Result<()> {
    if !path.exists() {
        return if enable {
            Err(Error::Config(
                "Start Cursor and sign in before enabling Nexusor BYOK".into(),
            ))
        } else {
            Ok(())
        };
    }
    let mut db = SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(path)
            .busy_timeout(std::time::Duration::from_secs(5)),
    )
    .await?;
    let mut tx = db.begin().await?;
    let raw: Option<String> =
        sqlx::query_scalar("SELECT CAST(value AS TEXT) FROM ItemTable WHERE key=?")
            .bind(USER)
            .fetch_optional(&mut *tx)
            .await?;
    let mut user = raw
        .as_deref()
        .map(serde_json::from_str::<Value>)
        .transpose()?
        .unwrap_or_else(|| json!({}));
    let obj = user
        .as_object_mut()
        .ok_or_else(|| Error::Config("Cursor applicationUser must be an object".into()))?;
    let backup: Option<String> =
        sqlx::query_scalar("SELECT CAST(value AS TEXT) FROM ItemTable WHERE key=?")
            .bind(BACKUP)
            .fetch_optional(&mut *tx)
            .await?;
    if enable {
        if backup.is_none() || obj.get("useOpenAIKey") != Some(&Value::Bool(true)) {
            let baseline =
                json!({"present":obj.contains_key("useOpenAIKey"),"value":obj.get("useOpenAIKey")});
            sqlx::query("INSERT OR REPLACE INTO ItemTable(key,value) VALUES(?,?)")
                .bind(BACKUP)
                .bind(baseline.to_string())
                .execute(&mut *tx)
                .await?;
        }
        obj.insert("useOpenAIKey".into(), Value::Bool(true));
    } else {
        let Some(backup) = backup else {
            return Ok(());
        };
        let baseline: Value = serde_json::from_str(&backup)?;
        if obj.get("useOpenAIKey") == Some(&Value::Bool(true)) {
            match baseline["present"].as_bool() {
                Some(true) => {
                    obj.insert("useOpenAIKey".into(), baseline["value"].clone());
                }
                Some(false) => {
                    obj.remove("useOpenAIKey");
                }
                None => {
                    return Err(Error::Config(
                        "Invalid Nexusor BYOK restoration record".into(),
                    ))
                }
            }
        }
        sqlx::query("DELETE FROM ItemTable WHERE key=?")
            .bind(BACKUP)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("INSERT OR REPLACE INTO ItemTable(key,value) VALUES(?,?)")
        .bind(USER)
        .bind(user.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn byok_roundtrip_preserves_account_other_preferences_and_user_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.vscdb");
        let mut db = SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
        sqlx::query("CREATE TABLE ItemTable(key TEXT PRIMARY KEY,value BLOB)")
            .execute(&mut db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO ItemTable VALUES('cursorAuth/accessToken','real-session')")
            .execute(&mut db)
            .await
            .unwrap();
        for original in [
            json!({"other":42}),
            json!({"useOpenAIKey":false,"other":42}),
            json!({"useOpenAIKey":true,"other":42}),
        ] {
            sqlx::query("INSERT OR REPLACE INTO ItemTable VALUES(?,?)")
                .bind(USER)
                .bind(original.to_string())
                .execute(&mut db)
                .await
                .unwrap();
            update(&path, true).await.unwrap();
            update(&path, true).await.unwrap();
            update(&path, false).await.unwrap();
            let raw: String = sqlx::query_scalar("SELECT value FROM ItemTable WHERE key=?")
                .bind(USER)
                .fetch_one(&mut db)
                .await
                .unwrap();
            assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(), original);
        }
        update(&path, true).await.unwrap();
        sqlx::query("UPDATE ItemTable SET value=? WHERE key=?")
            .bind(json!({"useOpenAIKey":false,"other":99}).to_string())
            .bind(USER)
            .execute(&mut db)
            .await
            .unwrap();
        update(&path, false).await.unwrap();
        let raw: String = sqlx::query_scalar("SELECT value FROM ItemTable WHERE key=?")
            .bind(USER)
            .fetch_one(&mut db)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&raw).unwrap(),
            json!({"useOpenAIKey":false,"other":99})
        );
        let token: String =
            sqlx::query_scalar("SELECT value FROM ItemTable WHERE key='cursorAuth/accessToken'")
                .fetch_one(&mut db)
                .await
                .unwrap();
        assert_eq!(token, "real-session");
        update(&path, true).await.unwrap();
        sqlx::query("UPDATE ItemTable SET value=? WHERE key=?")
            .bind(json!({"other":100}).to_string())
            .bind(USER)
            .execute(&mut db)
            .await
            .unwrap();
        update(&path, true).await.unwrap();
        update(&path, false).await.unwrap();
        let raw: String = sqlx::query_scalar("SELECT value FROM ItemTable WHERE key=?")
            .bind(USER)
            .fetch_one(&mut db)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&raw).unwrap(),
            json!({"other":100})
        );
    }
}
