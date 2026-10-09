//! First-launch storage checks without reading or modifying the user's profile.
use cursor_server::store::Store;

#[tokio::test]
async fn empty_install_creates_storage_and_reopens_without_accounts_or_models() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("nexusor.db");
    assert!(!database.exists());
    let url = format!("sqlite://{}", database.display());
    let store = Store::connect(&url).await.unwrap();
    assert!(database.is_file());
    assert!(store.models().await.unwrap().is_empty());
    assert!(!store.detailed_logging().await.unwrap());
    let ports = store.port_settings().await.unwrap();
    let desktop = store.desktop_settings().await.unwrap();
    let router = store.auto_router_config().await.unwrap();
    let migrations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert!(migrations > 0);
    store.pool().close().await;
    let reopened = Store::connect(&url).await.unwrap();
    assert!(reopened.models().await.unwrap().is_empty());
    assert_eq!(
        serde_json::to_value(reopened.port_settings().await.unwrap()).unwrap(),
        serde_json::to_value(ports).unwrap()
    );
    assert_eq!(
        serde_json::to_value(reopened.desktop_settings().await.unwrap()).unwrap(),
        serde_json::to_value(desktop).unwrap()
    );
    assert_eq!(
        serde_json::to_value(reopened.auto_router_config().await.unwrap()).unwrap(),
        serde_json::to_value(router).unwrap()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(reopened.pool())
            .await
            .unwrap(),
        migrations
    );
    reopened.pool().close().await;
}
