//! Listing, creating, reordering and removing model configurations.

use crate::model::{ModelConfig, ModelConfigInput, Overview};

use super::{types::*, ControlService, Result};

impl ControlService {
    pub async fn models(&self) -> Result<Vec<ModelConfig>> {
        self.store.models().await
    }

    pub async fn overview(
        &self,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
        model_hashes: Option<&str>,
        bucket_ms: Option<i64>,
    ) -> Result<Overview> {
        self.store
            .overview(start_ms, end_ms, model_hashes, bucket_ms)
            .await
    }

    pub async fn create_models(&self, models: &[ModelConfigInput]) -> Result<Vec<ModelConfig>> {
        self.store.create_models(models).await
    }

    pub async fn reorder_models(&self, model_hashes: &[String]) -> Result<Vec<ModelConfig>> {
        self.store.reorder_models(model_hashes).await
    }

    pub async fn delete_model(&self, model_hash: &str) -> Result<()> {
        self.store.delete_model(model_hash).await
    }

    pub async fn update_model(
        &self,
        model_hash: &str,
        input: &ModelConfigInput,
    ) -> Result<ModelConfig> {
        self.store.update_model(model_hash, input).await
    }

    pub async fn import_v0049_models(&self) -> Result<LegacyModelImportResult> {
        let path = crate::config::v0049_config_path()?;
        let outcome = self.store.import_v0049_model_config(&path).await?;
        Ok(LegacyModelImportResult {
            imported: outcome.imported,
            skipped: outcome.skipped,
            total: outcome.total,
        })
    }

    pub async fn preview_v0049_models(&self) -> Result<LegacyModelImportPreview> {
        let path = crate::config::v0049_config_path()?;
        let plan = self.store.preview_v0049_model_config(&path).await?;
        let total = plan.models.len();
        let existing_models = plan.models.iter().filter(|model| model.existing).count();
        Ok(LegacyModelImportPreview {
            source: path.display().to_string(),
            total,
            new_models: total - existing_models,
            existing_models,
            models: plan
                .models
                .into_iter()
                .map(|model| LegacyModelImportPreviewItem {
                    model_hash: model.model_hash,
                    display_name: model.input.display_name,
                    model_id: model.input.model_id,
                    model_type: model.input.model_type,
                    existing: model.existing,
                })
                .collect(),
        })
    }
}
