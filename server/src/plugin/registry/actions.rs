use super::*;

impl PluginRegistry {
    pub async fn resource_action(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
        action_id: &str,
        _input: serde_json::Value,
    ) -> Result<serde_json::Value> {
        if action_id == "update-note" {
            let note = _input
                .get("note")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim();
            self.inner
                .state
                .mutate_resource(plugin_id, resource_type, resource_id, |record| {
                    if let Some(obj) = record.private_data.as_object_mut() {
                        if note.is_empty() {
                            obj.remove("user_note");
                            obj.remove("userNote");
                            obj.remove("note");
                        } else {
                            obj.insert(
                                "user_note".into(),
                                serde_json::Value::String(note.to_string()),
                            );
                        }
                    }
                    Ok(())
                })
                .await?;

            return Ok(serde_json::json!({
                "title": "Not Güncellendi",
                "description": if note.is_empty() { "Hesap notu silindi." } else { "Hesap notu başarıyla kaydedildi." },
                "cards": []
            }));
        }

        if action_id == "ping-account" {
            let start = std::time::Instant::now();
            self.refresh_resource(plugin_id, resource_type, resource_id)
                .await?;
            let elapsed_ms = start.elapsed().as_millis();
            return Ok(serde_json::json!({
                "title": "Bağlantı Başarılı",
                "description": format!("Hesap bağlantısı ve token geçerliliği başarıyla doğrulandı ({elapsed_ms}ms)."),
                "cards": [
                    {
                        "id": "ping-result",
                        "title": "API Erişimi Aktif",
                        "status": "Sağlıklı",
                        "fields": [
                            { "id": "latency", "label": "Yanıt Süresi", "value": format!("{elapsed_ms}ms") },
                            { "id": "token_status", "label": "Token Durumu", "value": "Geçerli" }
                        ]
                    }
                ]
            }));
        }

        if action_id == "wakeup-account" {
            let msg = self
                .wakeup_resource(plugin_id, resource_type, resource_id)
                .await?;
            return Ok(serde_json::json!({
                "title": "Hesap Uyandırıldı",
                "description": msg,
                "cards": [
                    {
                        "id": "wakeup-success",
                        "title": "Uyandırma Başarılı",
                        "status": "Aktif",
                        "fields": [
                            { "id": "action", "label": "İşlem", "value": "Kota Sıfırlama Sayacı Tetiklendi" },
                            { "id": "status", "label": "Durum", "value": "Hazır / Canlı" }
                        ]
                    }
                ]
            }));
        }
        if (plugin_id == "dev.nexusor.examples.codex-auth"
            || plugin_id == "dev.cursorbyok.examples.codex-auth")
            && action_id == "redeem-reset-credit"
        {
            let mut records = self.inner.state.resources(plugin_id, resource_type).await?;
            let Some(record) = records.iter_mut().find(|r| r.id == resource_id) else {
                return Err(Error::RunNotFound(format!("resource {resource_id}")));
            };

            let client = self.client().await?;
            record.private_data =
                crate::provider::providers::account_private_data::normalize_account_private_data(
                    record.private_data.clone(),
                );
            let mut data: crate::provider::providers::codex::AccountData =
                serde_json::from_value(record.private_data.clone())
                    .map_err(|e| Error::Provider(format!("invalid account data: {e}")))?;

            let _ =
                crate::provider::providers::codex::ensure_fresh_account(&client, &mut data).await;

            let consumed_id = crate::provider::providers::codex::consume_reset_credit(
                &client,
                &data.access_token,
                data.account_id.as_deref(),
            )
            .await?;

            // Refresh account quota immediately
            let _ = self
                .refresh_resource(plugin_id, resource_type, resource_id)
                .await;

            return Ok(serde_json::json!({
                "title": "Sıfırlama Jetonu Başarıyla Kullanıldı",
                "description": format!("ChatGPT hesabı için {consumed_id} numaralı sıfırlama jetonu kullanıldı. Kullanım kotanız hemen %100 seviyesine sıfırlandı."),
                "cards": [
                    {
                        "id": "reset-success",
                        "title": "Kota Sıfırlandı",
                        "status": "Başarılı",
                        "fields": [
                            { "id": "credit_id", "label": "Kullanılan Jeton", "value": consumed_id },
                            { "id": "status", "label": "Durum", "value": "Aktif / Hazır" }
                        ]
                    }
                ]
            }));
        }

        Ok(serde_json::json!({ "status": "completed" }))
    }
}
