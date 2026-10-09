//! Row accounting and cleanup for disposable observability data.

use serde::{Deserialize, Serialize};

use crate::Result;

use super::Store;

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct StatisticsStorage {
    pub call_count: i64,
    pub trace_count: i64,
    pub database_bytes: i64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StatisticsStorageScope {
    #[default]
    Details,
    All,
}

impl Store {
    pub async fn statistics_storage(&self) -> Result<StatisticsStorage> {
        let (call_count, trace_count) = sqlx::query_as::<_, (i64, i64)>(
            "SELECT (SELECT COUNT(*) FROM llm_calls), (SELECT COUNT(*) FROM cursor_run_traces)",
        )
        .fetch_one(&self.pool)
        .await?;

        let page_count: i64 = sqlx::query_scalar("PRAGMA page_count")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        let page_size: i64 = sqlx::query_scalar("PRAGMA page_size")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(4096);
        let database_bytes = page_count.saturating_mul(page_size);

        Ok(StatisticsStorage {
            call_count,
            trace_count,
            database_bytes,
        })
    }

    pub fn start_background_maintenance(&self) {
        let store = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            loop {
                if let Err(err) = store.auto_prune_database(3).await {
                    tracing::debug!(%err, "background database maintenance encountered error");
                }
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            }
        });
    }

    pub async fn auto_prune_database(&self, max_age_days: i64) -> Result<()> {
        let cutoff_ms = crate::store::now_ms() - (max_age_days * 24 * 60 * 60 * 1000);
        let _write = self.writes.lock().await;

        let mut tx = self.pool.begin().await?;

        // 1. Delete expired request bodies and response chunks
        sqlx::query("DELETE FROM llm_call_requests WHERE rowid IN (SELECT r.rowid FROM llm_call_requests r JOIN llm_calls c ON r.call_id = c.call_id WHERE c.created_at_ms < ?)")
            .bind(cutoff_ms)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM llm_call_response_chunks WHERE rowid IN (SELECT rc.rowid FROM llm_call_response_chunks rc JOIN llm_calls c ON rc.call_id = c.call_id WHERE c.created_at_ms < ?)")
            .bind(cutoff_ms)
            .execute(&mut *tx)
            .await?;

        // 2. Delete expired trace artifacts and traces older than max_age_days
        sqlx::query("DELETE FROM cursor_run_trace_artifacts WHERE created_at_ms < ?")
            .bind(cutoff_ms)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM cursor_run_traces WHERE received_at_ms < ?")
            .bind(cutoff_ms)
            .execute(&mut *tx)
            .await?;

        // CAS blobs are also referenced from canonical message JSON and client
        // checkpoints. Absence from blob_edges does not make them disposable.

        tx.commit().await?;

        // Check freelist count: if free pages > 10MB or total size > 40MB, run VACUUM
        let page_count: i64 = sqlx::query_scalar("PRAGMA page_count")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        let page_size: i64 = sqlx::query_scalar("PRAGMA page_size")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(4096);
        let freelist_count: i64 = sqlx::query_scalar("PRAGMA freelist_count")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        let free_bytes = freelist_count * page_size;
        let total_bytes = page_count * page_size;

        if free_bytes > 10 * 1024 * 1024 || total_bytes > 40 * 1024 * 1024 {
            let _ = sqlx::query("VACUUM").execute(&self.pool).await;
            tracing::info!(
                free_bytes,
                total_bytes,
                "auto-prune: database automatically compacted via VACUUM"
            );
        }

        Ok(())
    }

    pub async fn clear_statistics_storage(&self) -> Result<StatisticsStorage> {
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await?;
        Self::clear_detail_storage_tx(&mut transaction).await?;
        transaction.commit().await?;
        let _ = sqlx::query("VACUUM").execute(&self.pool).await;
        self.statistics_storage().await
    }

    pub async fn clear_all_statistics_storage(&self) -> Result<StatisticsStorage> {
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await?;
        Self::clear_trace_artifacts_tx(&mut transaction).await?;
        sqlx::query("DELETE FROM llm_calls")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM cursor_run_traces")
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        let _ = sqlx::query("VACUUM").execute(&self.pool).await;
        self.statistics_storage().await
    }

    async fn clear_detail_storage_tx(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<()> {
        sqlx::query("DELETE FROM llm_call_requests")
            .execute(&mut **transaction)
            .await?;
        sqlx::query("DELETE FROM llm_call_response_chunks")
            .execute(&mut **transaction)
            .await?;
        Self::clear_trace_artifacts_tx(transaction).await
    }

    async fn clear_trace_artifacts_tx(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<()> {
        // Shared CAS payloads may still be needed by a resumed conversation.
        // Statistics cleanup only owns trace rows, not the referenced blobs.
        sqlx::query("DELETE FROM cursor_run_trace_artifacts")
            .execute(&mut **transaction)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn statistics_cleanup_preserves_conversation_blobs_across_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}", directory.path().join("test.db").display());
        let store = Store::connect(&url).await.unwrap();
        let payload = b"conversation image referenced by canonical message JSON";
        let id = store.put_blob(payload, &[]).await.unwrap();
        store.auto_prune_database(3).await.unwrap();
        assert_eq!(
            store.get_blob(&id).await.unwrap().as_deref(),
            Some(payload.as_slice())
        );
        store.clear_statistics_storage().await.unwrap();
        store.clear_all_statistics_storage().await.unwrap();
        store.pool.close().await;
        let reopened = Store::connect(&url).await.unwrap();
        assert_eq!(
            reopened.get_blob(&id).await.unwrap().as_deref(),
            Some(payload.as_slice())
        );
    }

    #[tokio::test]
    async fn storage_accounting_and_clearing_works_without_errors() {
        let directory = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}", directory.path().join("test.db").display());
        let store = Store::connect(&url).await.unwrap();

        let stats = store.statistics_storage().await.unwrap();
        assert_eq!(stats.call_count, 0);
        assert_eq!(stats.trace_count, 0);

        let cleared_details = store.clear_statistics_storage().await.unwrap();
        assert_eq!(cleared_details.call_count, 0);

        let cleared_all = store.clear_all_statistics_storage().await.unwrap();
        assert_eq!(cleared_all.call_count, 0);
    }
}
