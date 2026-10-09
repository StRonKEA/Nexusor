use super::*;

impl PluginRegistry {
    pub async fn remove(&self, plugin_id: &str) -> Result<()> {
        self.find_entry(Path::new("native"), plugin_id).await?;
        self.inner.state.clear(plugin_id).await
    }

    pub async fn pool_strategy(&self, plugin_id: &str) -> Result<PoolStrategy> {
        self.inner.state.pool_strategy(plugin_id).await
    }

    pub async fn set_pool_strategy(&self, plugin_id: &str, strategy: PoolStrategy) -> Result<()> {
        self.inner
            .state
            .set_pool_strategy(plugin_id, strategy)
            .await
    }

    pub(crate) fn extract_quota_reset_ms(private_data: &serde_json::Value) -> Option<i64> {
        let quota = private_data.get("quota")?;
        // 1. Antigravity: check weekly quotas first if exhausted, otherwise 5-hour rolling windows
        if let Some(reset) = quota
            .pointer("/gemini_weekly/reset_at_ms")
            .and_then(serde_json::Value::as_i64)
        {
            let rem = quota
                .pointer("/gemini_weekly/remaining_percent")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(100.0);
            if rem <= 1.0 && reset > 0 {
                return Some(reset);
            }
        }
        if let Some(reset) = quota
            .pointer("/claude_weekly/reset_at_ms")
            .and_then(serde_json::Value::as_i64)
        {
            let rem = quota
                .pointer("/claude_weekly/remaining_percent")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(100.0);
            if rem <= 1.0 && reset > 0 {
                return Some(reset);
            }
        }
        if let Some(reset) = quota
            .pointer("/gemini/reset_at_ms")
            .and_then(serde_json::Value::as_i64)
        {
            if reset > 0 {
                return Some(reset);
            }
        }
        if let Some(reset) = quota
            .pointer("/claude/reset_at_ms")
            .and_then(serde_json::Value::as_i64)
        {
            if reset > 0 {
                return Some(reset);
            }
        }
        // 2. Codex: check secondary (weekly) window if exhausted, then primary window
        if let Some(reset) = quota
            .pointer("/secondary_window/reset_at_ms")
            .and_then(serde_json::Value::as_i64)
        {
            let used = quota
                .pointer("/secondary_window/used_percent")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0);
            if used >= 100.0 && reset > 0 {
                return Some(reset);
            }
        }
        if let Some(reset) = quota
            .pointer("/primary_window/reset_at_ms")
            .and_then(serde_json::Value::as_i64)
        {
            if reset > 0 {
                return Some(reset);
            }
        }
        // 3. Grok / Claude Code / Kimi: quota.reset_at_ms
        if let Some(reset) = quota.get("reset_at_ms").and_then(serde_json::Value::as_i64) {
            if reset > 0 {
                return Some(reset);
            }
        }
        None
    }
}
