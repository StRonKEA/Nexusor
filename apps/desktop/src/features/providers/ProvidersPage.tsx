import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { PageContent } from "../../shell/layout/PageContent";
import { appStore, useAppStore } from "../../shared/store/appStore";
import { api, type Model, type ModelInput, type ModelType } from "../../shared/api";
import { Button } from "../../shared/ui/Button";
import { Card } from "../../shared/ui/Card";
import { Modal } from "../../shared/ui/Modal";
import { ConfirmDialog } from "../../shared/ui/ConfirmDialog";
import { useMessage } from "../../shared/ui/message";
import { getProviderLogo, providerLogos } from "../../shared/utils/providerIcons";
import {
  CursorModelEditor,
  emptyCursorModelDraft,
  type CursorModelDraft,
} from "../models/CursorModelEditor";
import styles from "./ProvidersPage.module.scss";

type ProviderGroup = {
  key: string;
  name: string;
  baseUrl: string;
  type: ModelType;
  models: Model[];
};

function parseHeaders(text: string): Record<string, string> {
  const parsed = parseObject(text, t("models.custom_headers"));
  if (Object.values(parsed).some((value) => typeof value !== "string")) {
    throw new Error(t("models.all_custom_header_values_must_be"));
  }
  return parsed as Record<string, string>;
}

function parseObject(text: string, label: string): Record<string, unknown> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text || "{}");
  } catch {
    throw new Error(t("models.must_be_valid_json", { label }));
  }
  if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
    throw new Error(t("models.must_be_a_json_object", { label }));
  }
  return parsed as Record<string, unknown>;
}

function draftInput(draft: CursorModelDraft): ModelInput {
  const group_name = draft.model.group_name?.trim() || null;
  if (!group_name) {
    throw new Error(t("models.provider_alias_required"));
  }
  const model = {
    ...draft.model,
    group_name,
    display_name: draft.model.display_name.trim(),
    base_url: draft.model.base_url.trim(),
    api_key: draft.model.api_key.trim(),
    tooltip_data: draft.model.tooltip_data.trim() || draft.model.display_name.trim(),
    model_id: draft.model.model_id.trim(),
    openai_extra_params: parseObject(draft.openAIExtraParamsText, t("models.openai_extra_parameters")),
    custom_headers: parseHeaders(draft.customHeadersText),
    anthropic_extra_params: parseObject(draft.anthropicExtraParamsText, t("models.anthropic_extra_parameters")),
  };
  if (!model.display_name || !model.base_url || !model.api_key || !model.model_id) {
    throw new Error(t("models.server_address_or_complete_reque"));
  }
  return model;
}

export function ProvidersPage() {
  const navigate = useNavigate();
  const message = useMessage();
  const { plugins, models, busy } = useAppStore();

  const [draft, setDraft] = useState<CursorModelDraft | null>(null);
  const [editingProvider, setEditingProvider] = useState<ProviderGroup | null>(null);
  const [deletingProvider, setDeletingProvider] = useState<ProviderGroup | null>(null);
  const [modelOptions, setModelOptions] = useState<string[]>([]);
  const [discovering, setDiscovering] = useState(false);
  const [saving, setSaving] = useState(false);

  const customProviders = useMemo(() => {
    const map = new Map<string, ProviderGroup>();
    for (const model of models) {
      const key = model.group_name?.trim() || model.base_url.trim();
      const existing = map.get(key);
      if (existing) {
        existing.models.push(model);
      } else {
        map.set(key, {
          key,
          name: model.group_name?.trim() || model.display_name || model.model_id,
          baseUrl: model.base_url,
          type: model.type,
          models: [model],
        });
      }
    }
    return Array.from(map.values());
  }, [models]);

  const openNewProvider = () => {
    setEditingProvider(null);
    const next = emptyCursorModelDraft();
    next.model.sort_order = models.length + 1;
    setModelOptions([]);
    setDraft(next);
  };

  const openEditForProvider = (provider: ProviderGroup) => {
    setEditingProvider(provider);
    const template = provider.models[0];
    const next = emptyCursorModelDraft();
    if (template) {
      next.model.base_url = template.base_url;
      next.model.api_key = template.api_key;
      next.model.type = template.type;
      next.model.group_name = template.group_name || provider.name;
      next.model.use_full_url = template.use_full_url;
      next.model.openai_endpoint = template.openai_endpoint;
      next.model.custom_headers_enabled = template.custom_headers_enabled;
      next.model.custom_headers = { ...template.custom_headers };
      next.model.model_id = template.model_id;
      next.model.display_name = template.display_name;
      next.model.tooltip_data = template.tooltip_data;
      next.customHeadersText = JSON.stringify(template.custom_headers, null, 2);
      next.openAIExtraParamsText = JSON.stringify(template.openai_extra_params || {}, null, 2);
      next.anthropicExtraParamsText = JSON.stringify(template.anthropic_extra_params || {}, null, 2);
      next.selectedModelIds = provider.models.map((m) => m.model_id);
    }
    setModelOptions([]);
    setDraft(next);
  };

  const openAddForProvider = (provider: ProviderGroup) => {
    const next = emptyCursorModelDraft();
    const template = provider.models[0];
    if (template) {
      next.model.base_url = template.base_url;
      next.model.api_key = template.api_key;
      next.model.type = template.type;
      next.model.group_name = template.group_name;
      next.model.use_full_url = template.use_full_url;
      next.model.custom_headers_enabled = template.custom_headers_enabled;
      next.model.custom_headers = { ...template.custom_headers };
      next.customHeadersText = JSON.stringify(template.custom_headers, null, 2);
    }
    next.model.sort_order = models.length + 1;
    setModelOptions([]);
    setDraft(next);
  };

  const discover = async (): Promise<boolean> => {
    if (!draft) return false;
    setDiscovering(true);
    try {
      const list = await api.discoverModels({
        type: draft.model.type,
        base_url: draft.model.base_url,
        api_key: draft.model.api_key,
        custom_headers_enabled: draft.model.custom_headers_enabled,
        custom_headers: parseHeaders(draft.customHeadersText),
      });
      if (list.models.length > 0) {
        setModelOptions(list.models);
        message(t("providers.discovered_models", { count: list.models.length }));
        return true;
      }
      message(t("providers.no_models_found"));
      return false;
    } catch (cause) {
      message(cause instanceof Error ? cause.message : String(cause));
      return false;
    } finally {
      setDiscovering(false);
    }
  };

  const saveCustomProvider = async () => {
    if (!draft) return;
    setSaving(true);
    try {
      const input = draftInput(draft);

      if (editingProvider) {
        for (const m of editingProvider.models) {
          await appStore.updateCursorModel(m.model_hash, {
            ...m,
            group_name: input.group_name,
            base_url: input.base_url,
            api_key: input.api_key,
            use_full_url: input.use_full_url,
            type: input.type,
            openai_endpoint: input.openai_endpoint,
            custom_headers_enabled: input.custom_headers_enabled,
            custom_headers: input.custom_headers,
            openai_extra_params: input.openai_extra_params,
            anthropic_extra_params: input.anthropic_extra_params,
          });
        }
        if (editingProvider.models.length === 1 && input.model_id) {
          const first = editingProvider.models[0];
          await appStore.updateCursorModel(first.model_hash, {
            ...first,
            ...input,
          });
        }
        message(t("providers.provider_saved"));
        setEditingProvider(null);
        setDraft(null);
        return;
      }

      const ids = draft.selectedModelIds && draft.selectedModelIds.length > 0
        ? draft.selectedModelIds
        : [input.model_id];
      const modelsToCreate = ids.map((id, idx) => ({
        ...input,
        model_id: id,
        display_name: ids.length === 1 ? input.display_name : id,
        sort_order: input.sort_order + idx,
      }));
      const created = await appStore.createModels(modelsToCreate);
      if (created) {
        message(t("providers.provider_saved"));
        setDraft(null);
      }
    } catch (cause) {
      message(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSaving(false);
    }
  };

  const handleDeleteProvider = async (provider: ProviderGroup) => {
    try {
      for (const m of provider.models) {
        await appStore.deleteModel(m.model_hash);
      }
      message(t("providers.provider_removed"));
    } catch (e) {
      message(e instanceof Error ? e.message : String(e));
    }
  };

  const content = (
    <div className={styles.container}>
      {/* Built-in OAuth / Device Code Providers */}
      <div className={styles.section}>
        <div className={styles.sectionHeader}>
          <div>
            <h3>{t("providers.built_in_providers_accounts")}</h3>
            <p className={styles.subtitle}>
              {t("providers.connect_google_chatgpt_and_xai_a")}
            </p>
          </div>
        </div>

        <div className={styles.grid}>
          {plugins.map((plugin) => {
            const configured = plugin.providers.some((p) => p.configured);
            const accountCount = plugin.resources.reduce((c, r) => c + r.resources.length, 0);
            const logo = getProviderLogo(plugin.id) || getProviderLogo(plugin.name);
            return (
              <Card key={plugin.id} className={styles.card}>
                <div className={styles.cardHeader}>
                  <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
                    {logo && <img src={logo} alt="" style={{ width: "26px", height: "26px", objectFit: "contain", borderRadius: "6px" }} />}
                    <h4 style={{ margin: 0, fontSize: "15px", fontWeight: 600, color: "#f7f8f8" }}>{plugin.name}</h4>
                  </div>
                  <span className={`${styles.badge} ${configured ? styles.badgeReady : ""}`}>
                    {configured ? t("providers.connected") : t("providers.not_connected")}
                  </span>
                </div>
                <p className={styles.metaText}>
                  {t("providers.active_account_s_connected", { count: accountCount })}
                </p>
                <div className={styles.cardActions}>
                  <Button
                    size="small"
                    variant={configured ? "secondary" : "primary"}
                    onClick={() =>
                      navigate(
                        `/providers/${encodeURIComponent(plugin.id)}?tab=${configured ? "accounts" : "add"}`
                      )
                    }
                  >
                    {configured ? t("providers.manage_accounts_2") : t("providers.connect_account")}
                  </Button>
                </div>
              </Card>
            );
          })}
        </div>
      </div>

      {/* Custom API Providers */}
      <div className={styles.section}>
        <div className={styles.sectionHeader}>
          <div>
            <h3>{t("providers.custom_api_providers")}</h3>
            <p className={styles.subtitle}>
              {t("providers.connect_deepseek_openrouter_olla")}
            </p>
          </div>
          <Button size="small" variant="primary" onClick={openNewProvider}>
            {t("providers.add_custom_api_provider")}
          </Button>
        </div>

        {customProviders.length === 0 ? (
          <Card className={styles.emptyCard}>
            <p>{t("providers.no_custom_api_providers_added_ye")}</p>
            <div style={{ marginTop: "1rem" }}>
              <Button size="small" variant="primary" onClick={openNewProvider}>
                {t("providers.add_custom_api_provider")}
              </Button>
            </div>
          </Card>
        ) : (
          <div className={styles.grid}>
            {customProviders.map((provider) => {
              const logo = getProviderLogo(provider.name) || getProviderLogo(provider.baseUrl) || (provider.type === "anthropic" ? providerLogos.anthropic : providerLogos.openai);
              return (
                <Card key={provider.key} className={styles.card}>
                  <div className={styles.cardHeader}>
                    <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
                      {logo && <img src={logo} alt="" style={{ width: "26px", height: "26px", objectFit: "contain", borderRadius: "6px" }} />}
                      <h4 style={{ margin: 0, fontSize: "15px", fontWeight: 600, color: "#f7f8f8" }}>{provider.name}</h4>
                    </div>
                    <span className={styles.badge}>
                      {provider.type === "anthropic" ? "Anthropic" : "OpenAI"}
                    </span>
                  </div>
                  <p className={styles.metaText}>
                    {provider.models.length} model:{" "}
                    {provider.models.map((m) => m.model_id).slice(0, 3).join(", ")}
                    {provider.models.length > 3 ? "..." : ""}
                  </p>
                  <div className={styles.cardActions} style={{ display: "flex", gap: "0.5rem" }}>
                    <Button
                      size="small"
                      variant="primary"
                      onClick={() => navigate("/models")}
                    >
                      {t("cursor.manage_models")}
                    </Button>
                    <Button
                      size="small"
                      variant="secondary"
                      onClick={() => openEditForProvider(provider)}
                    >
                      {t("common.edit")}
                    </Button>
                    <Button
                      size="small"
                      variant="secondary"
                      onClick={() => openAddForProvider(provider)}
                    >
                      {t("providers.add_model")}
                    </Button>
                    <Button
                      size="small"
                      variant="secondary"
                      onClick={() => setDeletingProvider(provider)}
                    >
                      {t("providers.remove")}
                    </Button>
                  </div>
                </Card>
              );
            })}
          </div>
        )}
      </div>

      {/* Provider / Model Editor Modal */}
      <Modal
        fullHeight
        open={draft !== null}
        title={editingProvider ? t("models.edit_model") : t("providers.add_custom_api_provider_2")}
        busy={busy || saving}
        onClose={() => { setDraft(null); setEditingProvider(null); }}
        onSubmit={() => void saveCustomProvider()}
        submitLabel={t("common.save")}
      >
        {draft && (
          <CursorModelEditor
            draft={draft}
            modelOptions={modelOptions}
            discovering={discovering}
            onChange={setDraft}
            onDiscover={discover}
            isEditing={Boolean(editingProvider)}
          />
        )}
      </Modal>
    </div>
  );

  return (
    <>
      <PageContent
        title={t("providers.providers_accounts")}
        sections={[{ key: "providers", estimatedHeight: 800, content }]}
      />

      <ConfirmDialog
        open={deletingProvider !== null}
        title={t("providers.delete_provider_confirm_title")}
        cancelLabel={t("common.cancel")}
        confirmLabel={t("common.delete")}
        onCancel={() => setDeletingProvider(null)}
        onConfirm={() => {
          if (deletingProvider) {
            void handleDeleteProvider(deletingProvider);
            setDeletingProvider(null);
          }
        }}
      >
        <p>
          {t("providers.delete_provider_confirm_desc", { name: deletingProvider?.name || "" })}
        </p>
      </ConfirmDialog>
    </>
  );
}
