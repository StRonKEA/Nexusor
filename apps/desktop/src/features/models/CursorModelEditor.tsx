import { useState } from "react";
import type { ModelInput, ModelType } from "../../shared/api";
import { defaultCustomHeadersText } from "../../shared/utils/modelDefaults";
import {
  modelPresets,
  presetEndpoint,
  trimTrailingSlash,
  type ModelPreset,
} from "../../shared/utils/modelPresets";
import { Button } from "../../shared/ui/Button";
import { Checkbox } from "../../shared/ui/Checkbox";
import { FormField, SecretTextInput, TextInput } from "../../shared/ui/FormControls";
import { JsonEditor } from "../../shared/ui/JsonEditor";
import { Select } from "../../shared/ui/Select";
import { Switch } from "../../shared/ui/Switch";
import { providerLogos } from "../../shared/utils/providerIcons";
import { CursorPresetChips } from "./CursorPresetChips";
import styles from "./CursorSettings.module.scss";

export type CursorModelDraft = {
  providerId: string;
  model: ModelInput;
  openAIExtraParamsText: string;
  customHeadersText: string;
  anthropicExtraParamsText: string;
  selectedModelIds?: string[];
};

export const emptyCursorModelDraft = (): CursorModelDraft => ({
  providerId: "builtin/openai",
  model: {
    sort_order: 0,
    display_name: "",
    group_name: null,
    type: "openai",
    base_url: "",
    use_full_url: false,
    api_key: "",
    tooltip_data: "",
    model_id: "",
    reasoning_effort: null,
    openai_endpoint: "/v1/chat/completions",
    openai_extra_params_enabled: false,
    openai_extra_params: {},
    custom_headers_enabled: false,
    custom_headers: {},
    anthropic_extra_params_enabled: false,
    anthropic_extra_params: {},
    context_window_tokens: null,
    max_completion_tokens: null,
    anthropic_max_tokens: null,
    anthropic_thinking_effort: "xhigh",
    thinking_budget_tokens: null,
  },
  openAIExtraParamsText: "{}",
  customHeadersText: defaultCustomHeadersText,
  anthropicExtraParamsText: "{}",
});

export function CursorModelEditor({
  draft,
  modelOptions,
  discovering,
  onChange,
  onDiscover,
  isEditing = false,
}: {
  draft: CursorModelDraft;
  modelOptions: string[];
  discovering: boolean;
  onChange: (draft: CursorModelDraft) => void;
  onDiscover: () => Promise<boolean>;
  isEditing?: boolean;
}) {
  const [filterQuery, setFilterQuery] = useState("");
  const selectedSet = new Set(draft.selectedModelIds || []);

  const setModel = (patch: Partial<ModelInput>) =>
    onChange({ ...draft, model: { ...draft.model, ...patch } });

  const setType = (type: ModelType) => {
    const other: ModelType = type === "anthropic" ? "openai" : "anthropic";
    const preset = modelPresets.find(
      (candidate) =>
        trimTrailingSlash(presetEndpoint(candidate, other).baseUrl) ===
        trimTrailingSlash(draft.model.base_url.trim())
    );
    const endpoint = preset ? presetEndpoint(preset, type) : null;
    onChange({
      ...draft,
      providerId: `builtin/${type}`,
      model: {
        ...draft.model,
        type,
        ...(endpoint
          ? {
              base_url: endpoint.baseUrl,
              use_full_url: endpoint.useFullUrl,
              custom_headers_enabled: endpoint.customHeaders !== null,
              custom_headers: endpoint.customHeaders ? { ...endpoint.customHeaders } : {},
            }
          : {}),
        openai_endpoint:
          type === "openai"
            ? endpoint?.openaiEndpoint || draft.model.openai_endpoint || "/v1/chat/completions"
            : "",
        anthropic_thinking_effort:
          type === "anthropic" ? draft.model.anthropic_thinking_effort || "xhigh" : null,
      },
      customHeadersText: endpoint?.customHeaders
        ? JSON.stringify(endpoint.customHeaders, null, 2)
        : draft.customHeadersText,
    });
  };

  const numberValue = (value: string) => (value === "" ? null : Math.trunc(Number(value)));
  const canDiscover = Boolean(draft.model.base_url.trim() && draft.model.api_key.trim());

  const presetModelOptions = modelPresets
    .filter(
      (preset) =>
        trimTrailingSlash(presetEndpoint(preset, draft.model.type).baseUrl) ===
        trimTrailingSlash(draft.model.base_url.trim())
    )
    .flatMap((preset) => preset.models.map((item) => item.model_id));
  const combinedOptions = [...new Set([...modelOptions, ...presetModelOptions])];
  const filteredOptions = combinedOptions.filter((id) =>
    id.toLowerCase().includes(filterQuery.toLowerCase())
  );

  const toggleModelId = (id: string) => {
    const nextSet = new Set(draft.selectedModelIds || []);
    if (nextSet.has(id)) {
      nextSet.delete(id);
    } else {
      nextSet.add(id);
    }
    const nextList = Array.from(nextSet);
    onChange({
      ...draft,
      selectedModelIds: nextList,
      model: {
        ...draft.model,
        model_id: nextList[0] || draft.model.model_id,
        display_name: draft.model.display_name || nextList[0] || "",
      },
    });
  };

  const selectAll = () => {
    const nextList = [...new Set([...(draft.selectedModelIds || []), ...filteredOptions])];
    onChange({
      ...draft,
      selectedModelIds: nextList,
      model: {
        ...draft.model,
        model_id: nextList[0] || draft.model.model_id,
        display_name: draft.model.display_name || nextList[0] || "",
      },
    });
  };

  const deselectAll = () => {
    onChange({
      ...draft,
      selectedModelIds: [],
    });
  };

  const discoverModels = async () => {
    await onDiscover();
  };

  const applyPreset = (preset: ModelPreset) => {
    const endpoint = presetEndpoint(preset, draft.model.type);
    const first = preset.models[0];
    const currentBase = trimTrailingSlash(draft.model.base_url.trim());
    const sameProvider = [preset.endpoints.anthropic, preset.endpoints.openai].some(
      (candidate) => trimTrailingSlash(candidate.baseUrl) === currentBase
    );
    onChange({
      ...draft,
      model: {
        ...draft.model,
        base_url: endpoint.baseUrl,
        use_full_url: endpoint.useFullUrl,
        openai_endpoint:
          draft.model.type === "openai" ? endpoint.openaiEndpoint : draft.model.openai_endpoint,
        custom_headers_enabled: endpoint.customHeaders !== null,
        custom_headers: endpoint.customHeaders ? { ...endpoint.customHeaders } : {},
        api_key: sameProvider ? draft.model.api_key : "",
        model_id: first?.model_id ?? draft.model.model_id,
        display_name: first?.display_name ?? draft.model.display_name,
        tooltip_data:
          !draft.model.tooltip_data.trim() || draft.model.tooltip_data === t("models.notes")
            ? preset.name
            : draft.model.tooltip_data,
        context_window_tokens: first?.context_window_tokens ?? draft.model.context_window_tokens,
        ...(draft.model.type === "openai"
          ? { max_completion_tokens: first?.max_output_tokens ?? draft.model.max_completion_tokens }
          : { anthropic_max_tokens: first?.max_output_tokens ?? draft.model.anthropic_max_tokens }),
      },
      customHeadersText: endpoint.customHeaders
        ? JSON.stringify(endpoint.customHeaders, null, 2)
        : draft.customHeadersText,
    });
  };

  const requestUrlPlaceholder = draft.model.use_full_url
    ? draft.model.type === "anthropic"
      ? "https://api.anthropic.com/v1/messages"
      : draft.model.openai_endpoint === "/v1/chat/completions"
        ? "https://api.openai.com/v1/chat/completions"
        : "https://api.openai.com/v1/responses"
    : draft.model.type === "anthropic"
      ? "https://api.anthropic.com"
      : "https://api.openai.com";

  return (
    <div className={styles.editor}>
      {/* 1. Protocol Selector */}
      <div className={styles.protocolTabs}>
        <button
          type="button"
          className={
            draft.model.type === "openai" ? styles.protocolTabActive : styles.protocolTab
          }
          onClick={() => setType("openai")}
        >
          <img
            src={providerLogos.openai}
            alt=""
            style={{ width: "16px", height: "16px", objectFit: "contain" }}
          />
          <span>OpenAI Protocol</span>
        </button>
        <button
          type="button"
          className={
            draft.model.type === "anthropic" ? styles.protocolTabActive : styles.protocolTab
          }
          onClick={() => setType("anthropic")}
        >
          <img
            src={providerLogos.anthropic}
            alt=""
            style={{ width: "16px", height: "16px", objectFit: "contain" }}
          />
          <span>Anthropic Protocol</span>
        </button>
      </div>

      {/* 2. Provider Presets */}
      <CursorPresetChips
        type={draft.model.type}
        baseUrl={draft.model.base_url}
        onPick={applyPreset}
      />

      <div className={styles.grid}>
        {/* 1. Request Protocol (OpenAI Auto/Chat/Responses or Anthropic Messages) */}
        <FormField
          label={t("models.request_protocol")}
          hint={t("models.only_determines_the_request_and_")}
        >
          {draft.model.type === "openai" ? (
            <Select
              ariaLabel={t("models.request_protocol")}
              value={draft.model.openai_endpoint}
              options={[
                { value: "/v1/chat/completions", label: "Auto (Chat / Fallback)" },
                { value: "/v1/responses", label: "Responses API (/v1/responses)" },
                { value: "/chat/completions", label: "Gateway (/chat/completions)" },
              ]}
              onChange={(openai_endpoint) => setModel({ openai_endpoint })}
            />
          ) : (
            <TextInput
              value="Messages API (/v1/messages)"
              disabled
            />
          )}
        </FormField>

        {/* 2. Provider Group / Alias (MANDATORY) */}
        <FormField
          label={`${t("models.provider_alias")} *`}
          hint={t("models.provider_alias_required_hint")}
        >
          <TextInput
            placeholder="e.g. DeepSeek, OpenRouter, Self-Hosted"
            value={draft.model.group_name ?? ""}
            onChange={(event) => setModel({ group_name: event.target.value || null })}
          />
        </FormField>

        {/* 3. Base URL / Server Address */}
        <div className={styles.urlField}>
          <FormField
            label={
              draft.model.use_full_url
                ? t("models.complete_request_url")
                : t("models.server_address")
            }
            hint={
              draft.model.use_full_url
                ? t("models.this_address_is_used_exactly_as_")
                : t("models.the_standard_endpoint_path_is_ap")
            }
          >
            <TextInput
              placeholder={requestUrlPlaceholder}
              value={draft.model.base_url}
              onChange={(event) => setModel({ base_url: event.target.value })}
            />
          </FormField>
          <Checkbox
            checked={draft.model.use_full_url}
            label={t("models.use_complete_request_url")}
            onChange={(use_full_url) => setModel({ use_full_url })}
          />
        </div>

        {/* 4. API Key */}
        <FormField label="API Key" hint={t("models.the_key_required_to_access_the_m")}>
          <SecretTextInput
            placeholder="sk-xxxxxx"
            autoComplete="off"
            value={draft.model.api_key}
            onChange={(event) => setModel({ api_key: event.target.value })}
          />
        </FormField>

        {/* 5. Model ID Section */}
        <div className={styles.fullWidth}>
          <FormField
            label={t("calls.model_name")}
            hint={t("models.enter_a_model_id_directly_or_loa")}
          >
            <div style={{ display: "flex", gap: "8px", width: "100%" }}>
              <TextInput
                placeholder="e.g. gpt-4o, claude-3-7-sonnet-20250219, deepseek-chat"
                value={draft.model.model_id}
                onChange={(e) =>
                  setModel({
                    model_id: e.target.value,
                    display_name: isEditing ? draft.model.display_name : e.target.value,
                  })
                }
              />
              <Button
                className={styles.discoverButton}
                disabled={discovering || !canDiscover}
                onClick={() => void discoverModels()}
              >
                {discovering ? t("models.fetching") : t("models.fetch_models")}
              </Button>
            </div>
          </FormField>

          {/* Discovered Multi-Select Panel */}
          {combinedOptions.length > 0 && (
            <div className={styles.discoveredBox}>
              <div className={styles.discoveredHeader}>
                <span>
                  {combinedOptions.length} {t("calls.model")}
                  {selectedSet.size > 0 && ` (${selectedSet.size} ${t("common.select_all")})`}
                </span>
                <div className={styles.discoveredActions}>
                  <input
                    type="text"
                    placeholder={t("models.filter_models")}
                    value={filterQuery}
                    onChange={(e) => setFilterQuery(e.target.value)}
                    className={styles.filterInput}
                  />
                  <Button size="small" variant="secondary" onClick={selectAll}>
                    {t("common.select_all")}
                  </Button>
                  {selectedSet.size > 0 && (
                    <Button size="small" variant="secondary" onClick={deselectAll}>
                      {t("common.clear")}
                    </Button>
                  )}
                </div>
              </div>
              <div className={styles.discoveredChips}>
                {filteredOptions.map((opt) => {
                  const active = selectedSet.has(opt);
                  return (
                    <button
                      type="button"
                      key={opt}
                      className={[styles.modelSelectChip, active && styles.modelSelectChipActive]
                        .filter(Boolean)
                        .join(" ")}
                      onClick={() => toggleModelId(opt)}
                    >
                      <span>{active ? "✓" : "+"}</span>
                      <span>{opt}</span>
                    </button>
                  );
                })}
              </div>
              {selectedSet.size > 1 && (
                <div className={styles.batchInfo}>
                  {t("models.batch_add_info", { count: selectedSet.size })}
                </div>
              )}
            </div>
          )}
        </div>

        {/* Display Name (Shown only when editing an existing model) */}
        {isEditing && (
          <FormField
            label={t("calls.display_name")}
            hint={t("models.used_only_for_display_and_does_n")}
          >
            <TextInput
              placeholder={t("models.for_example_primary_model")}
              value={draft.model.display_name}
              onChange={(event) => setModel({ display_name: event.target.value })}
            />
          </FormField>
        )}

        {/* 6. Notes / Tooltip */}
        <FormField
          className={styles.fullWidth}
          label={t("models.notes")}
          hint={t("models.shown_in_the_cursor_model_descri")}
        >
          <TextInput
            placeholder={t("models.enter_model_notes")}
            value={draft.model.tooltip_data}
            onChange={(event) => setModel({ tooltip_data: event.target.value })}
          />
        </FormField>

        {/* 11. Advanced Settings Accordion */}
        <details className={`${styles.fullWidth} ${styles.advanced}`}>
          <summary>{t("models.advanced")}</summary>
          <div className={styles.advancedBody}>
            <FormField
              label={t("models.context_window_tokens")}
              hint={t("models.leave_blank_to_use_the_default")}
            >
              <TextInput
                type="number"
                min={1}
                step={1}
                placeholder={t("models.leave_blank_to_use_the_default_2")}
                value={draft.model.context_window_tokens ?? ""}
                onChange={(event) =>
                  setModel({ context_window_tokens: numberValue(event.target.value) })
                }
              />
            </FormField>
            {draft.model.type === "openai" ? (
              <>
                <FormField
                  label={t("models.maximum_output_tokens")}
                  hint={t("models.leave_blank_to_use_the_default")}
                >
                  <TextInput
                    type="number"
                    min={1}
                    step={1}
                    placeholder={t("models.leave_blank_to_use_the_default_2")}
                    value={draft.model.max_completion_tokens ?? ""}
                    onChange={(event) =>
                      setModel({ max_completion_tokens: numberValue(event.target.value) })
                    }
                  />
                </FormField>
                <FormField label={t("models.reasoning_effort")}>
                  <Select
                    ariaLabel={t("models.reasoning_effort")}
                    value={draft.model.reasoning_effort ?? ""}
                    options={effortOptions(true)}
                    onChange={(value) => setModel({ reasoning_effort: value || null })}
                  />
                </FormField>
              </>
            ) : (
              <>
                <FormField
                  label={t("models.maximum_output_tokens")}
                  hint={t("models.leave_blank_to_use_the_default")}
                >
                  <TextInput
                    type="number"
                    min={1}
                    step={1}
                    placeholder={t("models.leave_blank_to_use_the_default_2")}
                    value={draft.model.anthropic_max_tokens ?? ""}
                    onChange={(event) =>
                      setModel({ anthropic_max_tokens: numberValue(event.target.value) })
                    }
                  />
                </FormField>
                <FormField label={t("calls.reasoning_effort")}>
                  <Select
                    ariaLabel={t("calls.reasoning_effort")}
                    value={draft.model.anthropic_thinking_effort ?? "xhigh"}
                    options={effortOptions(false)}
                    onChange={(anthropic_thinking_effort) =>
                      setModel({ anthropic_thinking_effort })
                    }
                  />
                </FormField>
                <FormField
                  label={t("models.thinking_budget_tokens")}
                  hint={t("models.leave_blank_to_use_adaptive_thin")}
                >
                  <TextInput
                    type="number"
                    min={1}
                    step={1}
                    placeholder={t("models.leave_blank_to_use_adaptive_thin_2")}
                    value={draft.model.thinking_budget_tokens ?? ""}
                    onChange={(event) =>
                      setModel({ thinking_budget_tokens: numberValue(event.target.value) })
                    }
                  />
                </FormField>
              </>
            )}

            <ToggleJsonField
              label={t("models.custom_headers")}
              enabled={draft.model.custom_headers_enabled}
              text={draft.customHeadersText}
              onEnabledChange={(custom_headers_enabled) =>
                setModel({ custom_headers_enabled })
              }
              onTextChange={(customHeadersText) => onChange({ ...draft, customHeadersText })}
            />
            {draft.model.type === "openai" ? (
              <ToggleJsonField
                label={t("models.openai_extra_parameters")}
                enabled={draft.model.openai_extra_params_enabled}
                text={draft.openAIExtraParamsText}
                onEnabledChange={(openai_extra_params_enabled) =>
                  setModel({ openai_extra_params_enabled })
                }
                onTextChange={(openAIExtraParamsText) =>
                  onChange({ ...draft, openAIExtraParamsText })
                }
              />
            ) : (
              <ToggleJsonField
                label={t("models.anthropic_extra_parameters")}
                enabled={draft.model.anthropic_extra_params_enabled}
                text={draft.anthropicExtraParamsText}
                onEnabledChange={(anthropic_extra_params_enabled) =>
                  setModel({ anthropic_extra_params_enabled })
                }
                onTextChange={(anthropicExtraParamsText) =>
                  onChange({ ...draft, anthropicExtraParamsText })
                }
              />
            )}
          </div>
        </details>
      </div>
    </div>
  );
}

function ToggleJsonField({
  label,
  enabled,
  text,
  onEnabledChange,
  onTextChange,
}: {
  label: string;
  enabled: boolean;
  text: string;
  onEnabledChange: (enabled: boolean) => void;
  onTextChange: (text: string) => void;
}) {
  return (
    <div className={`${styles.fullWidth} ${styles.jsonOption}`}>
      <label>
        <span>{label}</span>
        <Switch label={label} checked={enabled} onChange={onEnabledChange} />
      </label>
      {enabled && <JsonEditor ariaLabel={label} value={text} onChange={onTextChange} />}
    </div>
  );
}

function effortOptions(optional: boolean) {
  return [
    ...(optional ? [{ value: "", label: t("models.not_set") }] : []),
    { value: "low", label: "Low" },
    { value: "medium", label: "Medium" },
    { value: "high", label: "High" },
    { value: "xhigh", label: "Extra High" },
    { value: "max", label: "Max" },
  ];
}
