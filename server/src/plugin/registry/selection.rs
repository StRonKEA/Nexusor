use super::*;

impl PluginRegistry {
    pub async fn resources(
        &self,
        plugin_id: &str,
        resource_type: &str,
    ) -> Result<Vec<ResourceRecord>> {
        self.inner.state.resources(plugin_id, resource_type).await
    }

    pub async fn slot_quota(&self, slot: &crate::store::ComboSlot) -> f64 {
        let Some((plugin_id, provider_id, _)) = parse_model_id(&slot.model_id) else {
            return 0.0;
        };
        let Ok(entry) = self.find_entry(Path::new("native"), plugin_id).await else {
            return -1.0;
        };
        let Some(resource_type) = entry
            .definition
            .providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .and_then(|provider| provider.resource_type.as_deref())
        else {
            return -1.0;
        };
        let Ok(records) = self.resources(plugin_id, resource_type).await else {
            return -1.0;
        };
        records
            .iter()
            .filter(|record| {
                record.state.is_ready(now_ms())
                    && slot
                        .account_ids
                        .as_ref()
                        .is_none_or(|ids| ids.is_empty() || ids.contains(&record.id))
            })
            .map(extract_remaining_quota_percent)
            .fold(-1.0, f64::max)
    }

    pub async fn select_resource(
        &self,
        plugin_id: &str,
        resource_type: &str,
        excluded_ids: &[String],
    ) -> Result<ResourceRecord> {
        self.select_resource_filtered(plugin_id, resource_type, excluded_ids, None, None)
            .await
    }

    pub async fn select_resource_filtered(
        &self,
        plugin_id: &str,
        resource_type: &str,
        excluded_ids: &[String],
        allowed_ids: Option<&[String]>,
        conversation_id: Option<&str>,
    ) -> Result<ResourceRecord> {
        self.select_resource_with_strategy(
            plugin_id,
            resource_type,
            excluded_ids,
            allowed_ids,
            conversation_id,
            None,
        )
        .await
    }

    pub async fn select_resource_with_strategy(
        &self,
        plugin_id: &str,
        resource_type: &str,
        excluded_ids: &[String],
        allowed_ids: Option<&[String]>,
        conversation_id: Option<&str>,
        strategy_override: Option<PoolStrategy>,
    ) -> Result<ResourceRecord> {
        let records = self.inner.state.resources(plugin_id, resource_type).await?;
        let records: Vec<_> = if let Some(allowed) = allowed_ids {
            if allowed.is_empty() {
                records
            } else {
                records
                    .into_iter()
                    .filter(|r| allowed.contains(&r.id))
                    .collect()
            }
        } else {
            records
        };
        if records.is_empty() {
            return Err(Error::Provider(format!(
                "plugin '{plugin_id}' has no '{resource_type}' resource; add one first"
            )));
        }
        let now = now_ms();
        let ready_records: Vec<_> = records
            .iter()
            .filter(|record| record.state.is_ready(now) && !excluded_ids.contains(&record.id))
            .cloned()
            .collect();

        if !ready_records.is_empty() {
            // Apply circuit breaker: filter out accounts temporarily quarantined due to consecutive transient failures
            let breakers = self.inner.circuit_breakers.lock().await;
            let healthy_records: Vec<_> = ready_records
                .iter()
                .filter(|record| {
                    let key = format!("{plugin_id}:{resource_type}:{}", record.id);
                    if let Some(entry) = breakers.get(&key) {
                        now >= entry.quarantined_until_ms
                    } else {
                        true
                    }
                })
                .cloned()
                .collect();
            drop(breakers);

            let candidate_pool = if healthy_records.is_empty() {
                ready_records
            } else {
                healthy_records
            };

            // 1. Session Affinity: Check if conversation is already pinned to a healthy account
            if let Some(conv_id) =
                conversation_id.filter(|s| !s.trim().is_empty() && strategy_override.is_none())
            {
                let affinity_map = self.inner.conversation_affinity.lock().await;
                if let Some((pinned_resource_id, _)) = affinity_map.get(conv_id) {
                    if let Some(record) =
                        candidate_pool.iter().find(|r| &r.id == pinned_resource_id)
                    {
                        tracing::debug!(
                            conversation_id = conv_id,
                            account_id = %record.id,
                            "session affinity: reusing pinned account for conversation to preserve prompt cache"
                        );
                        return Ok(record.clone());
                    }
                }
            }

            let strategy = match strategy_override {
                Some(strategy) => strategy,
                None => self.pool_strategy(plugin_id).await?,
            };

            let selected = match strategy {
                PoolStrategy::MostQuota => {
                    // Sort accounts by highest remaining quota percentage first (e.g. 95% > 20%)
                    let mut sorted = candidate_pool;
                    sorted.sort_by(|a, b| {
                        let qa = extract_remaining_quota_percent(a);
                        let qb = extract_remaining_quota_percent(b);
                        qb.partial_cmp(&qa).unwrap_or(std::cmp::Ordering::Equal)
                    });

                    // If the top accounts have similar quotas (within 5%), rotate among them to avoid hot-spotting
                    let top_quota = extract_remaining_quota_percent(&sorted[0]);
                    let top_tier: Vec<_> = sorted
                        .into_iter()
                        .filter(|r| (top_quota - extract_remaining_quota_percent(r)).abs() <= 5.0)
                        .collect();

                    let mut cursors = self.inner.resource_cursors.lock().await;
                    let index = cursors
                        .entry(format!("{plugin_id}:{resource_type}:most_quota"))
                        .or_insert(0);
                    let choice = top_tier[*index % top_tier.len()].clone();
                    *index = (*index + 1) % top_tier.len();
                    choice
                }
                PoolStrategy::FillFirst => {
                    // OpenCodex Fill-First policy:
                    // Drains the primary account in the candidate pool until its quota drops to 0 or it cools,
                    // keeping all traffic on the first available account before reaching for backups.
                    candidate_pool[0].clone()
                }
                PoolStrategy::RoundRobin => {
                    let mut cursors = self.inner.resource_cursors.lock().await;
                    let index = cursors
                        .entry(format!("{plugin_id}:{resource_type}"))
                        .or_insert(0);
                    let selected = candidate_pool[*index % candidate_pool.len()].clone();
                    *index = (*index + 1) % candidate_pool.len();
                    selected
                }
                PoolStrategy::Failover | PoolStrategy::Single => candidate_pool[0].clone(),
            };

            // Pin conversation to newly selected account
            if let Some(conv_id) = conversation_id.filter(|s| !s.trim().is_empty()) {
                let mut affinity_map = self.inner.conversation_affinity.lock().await;
                let two_hours_ago = now - 2 * 3600 * 1000;
                affinity_map.retain(|_, (_, ts)| *ts > two_hours_ago);
                affinity_map.insert(conv_id.to_string(), (selected.id.clone(), now));
            }

            return Ok(selected);
        }

        // None of the accounts are ready right now! Check if any are cooling down
        let min_retry = records
            .iter()
            .filter_map(|r| match &r.state {
                crate::plugin::state::ResourceState::Cooling { retry_at_ms, .. } => *retry_at_ms,
                _ => None,
            })
            .min();

        let retry_msg = if let Some(retry_at) = min_retry {
            let wait_secs = ((retry_at - now).max(0)) / 1000;
            format!("cooling down (available in {wait_secs}s)")
        } else {
            "all accounts currently unavailable".to_string()
        };

        Err(Error::Provider(format!(
            "plugin '{plugin_id}' has no available '{resource_type}' resource; {retry_msg}"
        )))
    }
}
