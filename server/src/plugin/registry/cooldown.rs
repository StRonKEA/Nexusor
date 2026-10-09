use super::*;

impl PluginRegistry {
    /// Marks one provider account as cooling after a credential-scoped
    /// quota failure. Conductor semantics mirrored from CLIProxyAPI
    /// (`conductor_cooldown.go::MarkResult`): an in-flight cooldown is never
    /// shortened by a later failure on the same account.
    ///
    /// If the account's quota tracking contains an exact upstream `reset_at_ms`
    pub async fn cool_resource(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
        cooldown: std::time::Duration,
        message: &str,
    ) -> Result<()> {
        self.cool_resource_inner(
            plugin_id,
            resource_type,
            resource_id,
            cooldown,
            message,
            true,
        )
        .await
    }

    /// Cooling for a credential the upstream refused.
    ///
    /// Unlike a quota failure, this deliberately ignores the account's stored quota
    /// reset. A rejected token is not bound by the quota window: honouring a reset
    /// that is weeks away would park a perfectly refreshable account for weeks, which
    /// is the exact opposite of the self-healing this exists for.
    pub(crate) async fn cool_auth_resource(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
        cooldown: std::time::Duration,
        message: &str,
    ) -> Result<()> {
        self.cool_resource_inner(
            plugin_id,
            resource_type,
            resource_id,
            cooldown,
            message,
            false,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn cool_resource_inner(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
        cooldown: std::time::Duration,
        message: &str,
        honour_quota_reset: bool,
    ) -> Result<()> {
        let now = now_ms();
        self.inner
            .state
            .mutate_resource(plugin_id, resource_type, resource_id, |record| {
                if matches!(
                    record.state,
                    ResourceState::Disabled | ResourceState::Invalid { .. }
                ) {
                    return Ok(());
                }
                let existing_ready_at = match record.state {
                    ResourceState::Cooling { retry_at_ms, .. } => retry_at_ms,
                    _ => None,
                }
                .unwrap_or(0);

                // Safety buffer: wait an additional 60 seconds (60,000 ms) after the quota reset
                // to prevent premature calls right on the boundary second.
                const SAFETY_BUFFER_MS: i64 = 60_000;

                let target_retry_at_ms = if let Some(reset_ms) =
                    Self::extract_quota_reset_ms(&record.private_data)
                        .filter(|&ms| ms > now)
                        .filter(|_| honour_quota_reset)
                {
                    // Upstream reset timestamp + 60s safety buffer
                    reset_ms.saturating_add(SAFETY_BUFFER_MS)
                } else {
                    // Dynamic cooldown duration + 60s safety buffer
                    now.saturating_add(cooldown.as_millis().min(i64::MAX as u128) as i64)
                        .saturating_add(SAFETY_BUFFER_MS)
                };

                if existing_ready_at > target_retry_at_ms {
                    return Ok(());
                }
                record.state = ResourceState::Cooling {
                    retry_at_ms: Some(target_retry_at_ms),
                    message: Some(message.to_owned()),
                };
                Ok(())
            })
            .await
    }

    /// Antigravity quotas are shared within a model family, not across the account.
    pub(crate) async fn cool_model_resource(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
        model_id: &str,
        cooldown: std::time::Duration,
    ) -> Result<()> {
        use crate::provider::providers::quota::antigravity_model_family;
        let family = (provider_id_for_resource(plugin_id, resource_type) == Some("antigravity"))
            .then(|| antigravity_model_family(model_id))
            .flatten();
        let Some(family) = family else {
            return self
                .cool_resource(
                    plugin_id,
                    resource_type,
                    resource_id,
                    cooldown,
                    "upstream quota exhausted",
                )
                .await;
        };
        let now = now_ms();
        self.inner
            .state
            .mutate_resource(plugin_id, resource_type, resource_id, |record| {
                if matches!(
                    record.state,
                    ResourceState::Disabled | ResourceState::Invalid { .. }
                ) {
                    return Ok(());
                }
                let mut retry_at = now
                    .saturating_add(cooldown.as_millis().min(i64::MAX as u128) as i64)
                    .saturating_add(60_000);
                // Only exhausted windows of this family can extend its backoff.
                // Healthy/stale windows must not replace the error's retry delay.
                if let Some(quota) = record.private_data.get("quota") {
                    for (window, threshold) in
                        [(family.to_owned(), 0.0), (format!("{family}_weekly"), 1.0)]
                    {
                        if let Some(value) = quota.get(window) {
                            if value
                                .get("remaining_percent")
                                .and_then(serde_json::Value::as_f64)
                                .is_some_and(|p| p <= threshold)
                            {
                                if let Some(reset) = value
                                    .get("reset_at_ms")
                                    .and_then(serde_json::Value::as_i64)
                                    .filter(|t| *t > now)
                                {
                                    retry_at = retry_at.max(reset.saturating_add(60_000));
                                }
                            }
                        }
                    }
                }
                let data = record.private_data.as_object_mut().ok_or_else(|| {
                    Error::Protocol("Antigravity account data must be an object".into())
                })?;
                let cooldowns = data
                    .entry("modelFamilyCooldowns")
                    .or_insert_with(|| serde_json::json!({}))
                    .as_object_mut()
                    .ok_or_else(|| {
                        Error::Protocol("modelFamilyCooldowns must be an object".into())
                    })?;
                let previous = cooldowns
                    .get(family)
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                cooldowns.insert(family.into(), serde_json::json!(retry_at.max(previous)));
                Ok(())
            })
            .await
    }

    pub async fn report_circuit_breaker_failure(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
    ) {
        let key = format!("{plugin_id}:{resource_type}:{resource_id}");
        let mut breakers = self.inner.circuit_breakers.lock().await;
        // Without this the map grows by one entry per distinct resource id that
        // ever failed, for the lifetime of the process.
        let stale_before = now_ms() - CIRCUIT_BREAKER_RETENTION_MS;
        breakers.retain(|_, entry| entry.quarantined_until_ms > stale_before);
        let entry = breakers.entry(key).or_insert(CircuitBreakerEntry {
            consecutive_failures: 0,
            quarantined_until_ms: 0,
        });
        entry.consecutive_failures += 1;
        if entry.consecutive_failures >= 2 {
            let quarantine_ms = now_ms() + 30_000;
            entry.quarantined_until_ms = quarantine_ms;
            tracing::warn!(
                plugin = %plugin_id,
                resource = %resource_id,
                consecutive = entry.consecutive_failures,
                "circuit breaker: account temporarily quarantined for 30s after consecutive upstream errors"
            );
        }
    }

    pub async fn report_circuit_breaker_success(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
    ) {
        let key = format!("{plugin_id}:{resource_type}:{resource_id}");
        let mut breakers = self.inner.circuit_breakers.lock().await;
        if let Some(entry) = breakers.get_mut(&key) {
            entry.consecutive_failures = 0;
            entry.quarantined_until_ms = 0;
        }
    }
}
