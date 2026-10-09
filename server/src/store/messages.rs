//! Persists canonical Messages and enforces event idempotency.
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    model::{CanonicalMessage, ConversationId},
    Error, Result,
};

use super::{now_ms, Store};

impl Store {
    /// Cursor can roll back past the active tool on Stop. Retain its observations
    /// outside that branch until the next run commits them.
    pub(crate) async fn save_interrupted_output(
        &self,
        conversation_id: &ConversationId,
        message: &CanonicalMessage,
        run_id: &crate::model::RunId,
        source_user_id: &str,
        source_user_text: &str,
    ) -> Result<()> {
        let _write = self.writes.lock().await;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        Self::ensure_conversation_tx(&mut tx, conversation_id).await?;
        Self::put_message_tx(&mut tx, conversation_id, message).await?;
        let head: i64 = sqlx::query_scalar(
            "SELECT head_checkpoint_id FROM runs WHERE run_id = ? AND conversation_id = ?",
        )
        .bind(run_id.as_str())
        .bind(conversation_id.as_str())
        .fetch_one(&mut *tx)
        .await?;
        let source_prefix = format!("runtime:cursor:user:{source_user_id}:");
        let source_message_id = Self::load_checkpoint_messages_tx(&mut tx, head)
            .await?
            .into_iter()
            .rev()
            .find(|message| message.message_id.starts_with(&source_prefix))
            .map(|message| message.message_id);
        sqlx::query(
            "INSERT OR IGNORE INTO interrupted_outputs
            (conversation_id, message_id, source_user_id, source_user_text, source_message_id)
            VALUES (?, ?, ?, ?, ?)",
        )
        .bind(conversation_id.as_str())
        .bind(&message.message_id)
        .bind(source_user_id)
        .bind(source_user_text)
        .bind(source_message_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn pending_interrupted_output(
        &self,
        conversation_id: &ConversationId,
        user_id: &str,
        user_text: &str,
        base_messages: &[CanonicalMessage],
    ) -> Result<Vec<CanonicalMessage>> {
        let rows = sqlx::query(
            "SELECT m.payload_json, i.source_user_id, i.source_user_text, i.source_message_id
             FROM messages m JOIN interrupted_outputs i
             ON i.conversation_id = m.conversation_id AND i.message_id = m.message_id
             WHERE m.conversation_id = ?
             AND NOT EXISTS (SELECT 1 FROM checkpoint_messages cm
                 WHERE cm.conversation_id = m.conversation_id AND cm.message_id = m.message_id)
             ORDER BY m.created_at_ms, m.message_id",
        )
        .bind(conversation_id.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .filter(|row| {
                let source_id: &str = row.get("source_user_id");
                let source_text: &str = row.get("source_user_text");
                let source_message: Option<&str> = row.get("source_message_id");
                // Cursor can merge the stopped input and follow-up under the same ID.
                // An edited input must not inherit observations from the discarded branch.
                if source_id == user_id {
                    !source_text.is_empty() && user_text.starts_with(source_text)
                } else {
                    source_message.is_some_and(|id| {
                        base_messages.iter().any(|message| message.message_id == id)
                    })
                }
            })
            .map(|row| serde_json::from_str(row.get::<&str, _>("payload_json")).map_err(Into::into))
            .collect()
    }

    pub async fn message(
        &self,
        conversation_id: &ConversationId,
        message_id: &str,
    ) -> Result<Option<CanonicalMessage>> {
        let payload: Option<String> = sqlx::query_scalar(
            "SELECT payload_json FROM messages WHERE conversation_id = ? AND message_id = ?",
        )
        .bind(conversation_id.as_str())
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;
        payload
            .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
            .transpose()
    }

    pub(crate) async fn put_message_tx(
        tx: &mut Transaction<'_, Sqlite>,
        conversation_id: &ConversationId,
        message: &CanonicalMessage,
    ) -> Result<()> {
        let payload = serde_json::to_string(message)?;
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO messages
             (conversation_id, message_id, role, origin, payload_json, runtime_event_id, created_at_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(conversation_id.as_str())
        .bind(&message.message_id)
        .bind(role_name(&message.role))
        .bind(origin_name(&message.origin))
        .bind(&payload)
        .bind(&message.runtime_event_id)
        .bind(now_ms())
        .execute(&mut **tx)
        .await?
        .rows_affected()
            == 1;
        if inserted {
            return Ok(());
        }

        let existing: Option<String> = sqlx::query_scalar(
            "SELECT payload_json FROM messages
             WHERE conversation_id = ? AND (message_id = ? OR runtime_event_id = ?)",
        )
        .bind(conversation_id.as_str())
        .bind(&message.message_id)
        .bind(&message.runtime_event_id)
        .fetch_optional(&mut **tx)
        .await?;
        match existing {
            Some(existing) if existing == payload => Ok(()),
            Some(_) => Err(Error::Store(format!(
                "message id or runtime event reused with different content: {}",
                message.message_id
            ))),
            None => Err(Error::Store(format!(
                "message insert was ignored without an existing object: {}",
                message.message_id
            ))),
        }
    }

    pub(crate) async fn load_checkpoint_messages_tx(
        tx: &mut Transaction<'_, Sqlite>,
        checkpoint_id: i64,
    ) -> Result<Vec<CanonicalMessage>> {
        let rows = sqlx::query(
            "WITH RECURSIVE lineage(checkpoint_id, parent_checkpoint_id, depth) AS (
                 SELECT checkpoint_id, parent_checkpoint_id, 0
                 FROM conversation_checkpoints WHERE checkpoint_id = ?
                 UNION ALL
                 SELECT r.checkpoint_id, r.parent_checkpoint_id, lineage.depth + 1
                 FROM conversation_checkpoints r
                 JOIN lineage ON r.checkpoint_id = lineage.parent_checkpoint_id
             )
             SELECT m.payload_json
             FROM lineage
             JOIN checkpoint_messages rm ON rm.checkpoint_id = lineage.checkpoint_id
             JOIN messages m
               ON m.conversation_id = rm.conversation_id AND m.message_id = rm.message_id
             ORDER BY lineage.depth DESC, rm.ordinal ASC",
        )
        .bind(checkpoint_id)
        .fetch_all(&mut **tx)
        .await?;
        rows.into_iter()
            .map(|row| serde_json::from_str(row.get::<&str, _>(0)).map_err(Into::into))
            .collect()
    }
}

fn role_name(role: &crate::model::Role) -> &'static str {
    match role {
        crate::model::Role::System => "system",
        crate::model::Role::User => "user",
        crate::model::Role::Assistant => "assistant",
        crate::model::Role::Tool => "tool",
    }
}

fn origin_name(origin: &crate::model::Origin) -> &'static str {
    match origin {
        crate::model::Origin::Prompt => "prompt",
        crate::model::Origin::User => "user",
        crate::model::Origin::Runtime => "runtime",
        crate::model::Origin::Assistant => "assistant",
        crate::model::Origin::Tool => "tool",
    }
}
