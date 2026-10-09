import { useCallback, useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { api, configuredPluginModels, type Model, type ModelInput } from "../../shared/api";
import { CursorModelCards, cursorModelGroups, type CursorModelGroup, type CursorModelGrouping } from "./CursorModelCards";
import { CursorModelEditor, emptyCursorModelDraft, type CursorModelDraft } from "./CursorModelEditor";
import { CursorModelGate, CursorModelProvider } from "./CursorGates";
import { CursorModelTestResult, type CursorModelTestState } from "./CursorModelTestResult";
import styles from "./CursorSettings.module.scss";
import { PageContent } from "../../shell/layout/PageContent";
import { ConfirmDialog } from "../../shared/ui/ConfirmDialog";
import { FormField, SecretTextInput, TextInput } from "../../shared/ui/FormControls";
import controls from "../../shared/ui/Controls.module.scss";
import { Modal } from "../../shared/ui/Modal";
import { useMessage } from "../../shared/ui/message";
import { PageActions } from "../../shell/PageActions";
import { appStore, useAppStore } from "../../shared/store/appStore";
import { errorText } from "../../shared/utils/errorText";

export function CursorSettingsPage() {
  const { models, cursorBusy, plugins } = useAppStore();
  const navigate = useNavigate();
  const location = useLocation();
  const message = useMessage();
  const [draft, setDraft] = useState<CursorModelDraft | null>(null);
  const [editing, setEditing] = useState<Model | null>(null);
  const [modelOptions, setModelOptions] = useState<string[]>([]);
  const [discovering, setDiscovering] = useState(false);
  const [deleting, setDeleting] = useState<Model | null>(null);
  const [testingModelHashes, setTestingModelHashes] = useState<Set<string>>(() => new Set());
  const [modelTestResults, setModelTestResults] = useState<Map<string, CursorModelTestState>>(() => new Map());
  const [savingAndTesting, setSavingAndTesting] = useState(false);
  const [grouping, setGrouping] = useState<CursorModelGrouping>("provider");
  const [settingsGroup, setSettingsGroup] = useState<CursorModelGroup | null>(null);
  const [groupNameDraft, setGroupNameDraft] = useState("");
  const [groupBaseUrlDraft, setGroupBaseUrlDraft] = useState("");
  const [groupApiKeyDraft, setGroupApiKeyDraft] = useState("");
  const [groupSettingsBusy, setGroupSettingsBusy] = useState(false);
  const activeModelTests = useRef(new Map<string, { testId: string; controller: AbortController; cancelling: boolean }>());
  const pluginModels = configuredPluginModels(plugins);
  const providerGroups = cursorModelGroups(models, "provider");
  const typeGroups = cursorModelGroups(models, "type");
  const canGroupByProvider = providerGroups.length > 1;
  const canGroupByType = typeGroups.length > 1;

  useEffect(() => {
    if ((grouping === "provider" && !canGroupByProvider) || (grouping === "type" && !canGroupByType)) {
      setGrouping("flat");
    }
  }, [canGroupByProvider, canGroupByType, grouping]);

  const openNew = () => {
    const next = emptyCursorModelDraft();
    next.model.sort_order = models.length + 1;
    setEditing(null);
    setModelOptions([]);
    setDraft(next);
  };
  const openEdit = (model: Model) => {
    setEditing(model);
    setModelOptions([model.model_id]);
    setDraft({
      providerId: `builtin/${model.type}`,
      model: modelInput(model),
      openAIExtraParamsText: JSON.stringify(model.openai_extra_params, null, 2),
      customHeadersText: JSON.stringify(model.custom_headers, null, 2),
      anthropicExtraParamsText: JSON.stringify(model.anthropic_extra_params, null, 2),
    });
  };
  // 处理来自 Providers 页面的"新增 / 编辑"跳转。
  // 由于 KeepAlive 页面可能不会重新挂载,因此监听 location.state 的变化。
  useEffect(() => {
    const state = location.state as { openModelEditor?: boolean; editModelHash?: string } | null;
    if (!state) return;
    if (state.openModelEditor) {
      openNew();
    } else if (state.editModelHash) {
      const target = models.find((m) => m.model_hash === state.editModelHash);
      if (target) {
        setEditing(target);
        setModelOptions([target.model_id]);
        setDraft({
          providerId: `builtin/${target.type}`,
          model: modelInput(target),
          openAIExtraParamsText: JSON.stringify(target.openai_extra_params, null, 2),
          customHeadersText: JSON.stringify(target.custom_headers, null, 2),
          anthropicExtraParamsText: JSON.stringify(target.anthropic_extra_params, null, 2),
        });
      }
    }
    navigate(location.pathname, { replace: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [location.state]);
  const discover = async (): Promise<boolean> => {
    if (!draft) return false;
    setDiscovering(true);
    try {
      const custom_headers = parseHeaders(draft.customHeadersText);
      const result = await api.discoverModels({
        type: draft.model.type,
        base_url: draft.model.base_url.trim(),
        api_key: draft.model.api_key.trim(),
        custom_headers_enabled: draft.model.custom_headers_enabled,
        custom_headers,
      });
      setModelOptions([...new Set(result.models)]);
      return true;
    } catch (cause) {
      message(errorText(cause));
      return false;
    } finally {
      setDiscovering(false);
    }
  };
  const persist = async (): Promise<Model | null> => {
    if (!draft) return null;
    const input = draftInput(draft);
    if (editing) return appStore.updateCursorModel(editing.model_hash, input);
    const ids = draft.selectedModelIds && draft.selectedModelIds.length > 0
      ? draft.selectedModelIds
      : [input.model_id];
    const inputs = ids.map((id, idx) => ({
      ...input,
      model_id: id,
      display_name: ids.length === 1 ? input.display_name : id,
      sort_order: input.sort_order + idx,
    }));
    return (await appStore.createModels(inputs))?.[0] ?? null;
  };
  const save = async () => {
    try {
      if (await persist()) {
        setDraft(null);
        setEditing(null);
      }
    } catch (cause) {
      message(errorText(cause));
    }
  };
  const cancelModelTest = async (modelHash: string) => {
    const active = activeModelTests.current.get(modelHash);
    if (!active || active.cancelling) return;
    active.cancelling = true;
    active.controller.abort();
    try {
      await api.cancelModelTest(modelHash, active.testId);
    } catch (cause) {
      message(t("models.failed_to_cancel_test", { error: errorText(cause) }), { duration: 5000 });
    }
  };
  const testModel = async (model: { model_hash: string; display_name: string }, notify = true): Promise<"success" | "failure" | "cancelled"> => {
    if (activeModelTests.current.has(model.model_hash)) {
      await cancelModelTest(model.model_hash);
      return "cancelled";
    }
    const active = { testId: crypto.randomUUID(), controller: new AbortController(), cancelling: false };
    activeModelTests.current.set(model.model_hash, active);
    setTestingModelHashes((current) => new Set(current).add(model.model_hash));
    try {
      const result = await api.testModel(model.model_hash, active.testId, active.controller.signal);
      setModelTestResults((current) => new Map(current).set(model.model_hash, { status: "success", result }));
      if (notify) message(t("models.connectivity_test_for_succeeded_", { model: model.display_name, duration: result.duration_ms }));
      return "success";
    } catch (cause) {
      if (active.cancelling || active.controller.signal.aborted) {
        setModelTestResults((current) => new Map(current).set(model.model_hash, { status: "cancelled" }));
        return "cancelled";
      }
      const error = errorText(cause);
      setModelTestResults((current) => new Map(current).set(model.model_hash, { status: "error", error }));
      if (notify) message(t("models.connectivity_test_failed", { error }), { duration: 5000 });
      return "failure";
    } finally {
      if (activeModelTests.current.get(model.model_hash) === active) activeModelTests.current.delete(model.model_hash);
      setTestingModelHashes((current) => {
        const next = new Set(current);
        next.delete(model.model_hash);
        return next;
      });
    }
  };
  const saveAndTest = async () => {
    setSavingAndTesting(true);
    let saved: Model | null = null;
    try {
      saved = await persist();
    } catch (cause) {
      message(errorText(cause));
    } finally {
      setSavingAndTesting(false);
    }
    if (!saved) return;
    setEditing(saved);
    await testModel(saved);
    await appStore.refresh();
  };
  const duplicateModel = async (model: Model) => {
    const names = new Set(models.map((item) => item.display_name));
    const baseName = t("models.copy", { name: model.display_name });
    let displayName = baseName;
    let suffix = 2;
    while (names.has(displayName)) {
      displayName = `${baseName} ${suffix}`;
      suffix += 1;
    }
    const created = await appStore.createModels([{
      ...modelInput(model),
      sort_order: models.length + 1,
      display_name: displayName,
    }]);
    if (created) message(t("models.model_duplicated"));
  };
  const openGroupSettings = (group: CursorModelGroup) => {
    setGroupNameDraft(group.models.find((model) => model.group_name?.trim())?.group_name?.trim() ?? "");
    setGroupBaseUrlDraft(sharedValue(group.models.map((model) => model.base_url)) ?? "");
    setGroupApiKeyDraft(sharedValue(group.models.map((model) => model.api_key)) ?? "");
    setSettingsGroup(group);
  };
  const saveGroupSettings = async () => {
    if (!settingsGroup) return;
    const group_name = groupNameDraft.trim() || null;
    const base_url = groupBaseUrlDraft.trim();
    const api_key = groupApiKeyDraft.trim();
    setGroupSettingsBusy(true);
    try {
      for (const model of settingsGroup.models) {
        const input: ModelInput = {
          ...modelInput(model),
          group_name,
          ...(base_url ? { base_url } : {}),
          ...(api_key ? { api_key } : {}),
        };
        if (input.group_name === (model.group_name ?? null)
          && input.base_url === model.base_url
          && input.api_key === model.api_key) continue;
        await api.updateModel(model.model_hash, input);
      }
      await appStore.refresh();
      setSettingsGroup(null);
    } catch (cause) {
      message(errorText(cause));
    } finally {
      setGroupSettingsBusy(false);
    }
  };
  const reorderModels = useCallback(async (modelHashes: string[]) => {
    if (!await appStore.reorderCursorModels(modelHashes)) {
      message(appStore.getSnapshot().error || t("models.unable_to_save_model_order"));
    }
  }, [message]);

  const list = <CursorModelCards
    models={models}
    pluginModels={pluginModels}
    plugins={plugins}
    grouping={grouping}
    disabled={cursorBusy}
    testingModelHashes={testingModelHashes}
    testResults={modelTestResults}
    onTest={(model) => void testModel(model)}
    onEdit={openEdit}
    onDuplicate={(model) => void duplicateModel(model)}
    onDelete={setDeleting}
    onTestPluginModel={(model) => void testModel({ model_hash: model.id, display_name: model.displayName })}
    onPluginSettings={(model) => navigate(`/providers/${encodeURIComponent(model.pluginId)}`)}
    onReorder={reorderModels}
    onGroupSettings={openGroupSettings}
  />;

  const content = <div className={styles.page}>
    <CursorModelProvider>
      <CursorModelGate busy={cursorBusy} onNavigateToProviders={() => navigate("/providers")}>
        {list}
      </CursorModelGate>
    </CursorModelProvider>
  </div>;

  const editorTestState = editing ? modelTestResults.get(editing.model_hash) : undefined;
  const editorTesting = Boolean(editing && testingModelHashes.has(editing.model_hash));
  const activeGroups = grouping === "provider" ? providerGroups : typeGroups;
  const pluginSectionHeight = pluginModels.length > 0 ? 60 + pluginModels.length * 56 : 0;
  const estimatedModelHeight = grouping === "flat"
    ? Math.max(380, Math.ceil(models.length / 3) * 196 + pluginSectionHeight)
    : Math.max(380, activeGroups.reduce((height, group) => height + 60 + group.models.length * 56, 0) + Math.max(0, activeGroups.length - 1) * 20 + pluginSectionHeight);

  return <>
    <PageActions position="left">
      <div className={styles.takeoverActions}>
        <div className={styles.groupActions} role="group" aria-label={t("calls.actions")}>
          {canGroupByProvider && <button type="button" aria-pressed={grouping === "provider"} onClick={() => setGrouping("provider")}>{t("models.by_provider")}</button>}
          <button type="button" aria-pressed={grouping === "flat"} onClick={() => setGrouping("flat")}>{t("models.default_layout")}</button>
        </div>
      </div>
    </PageActions>
    <PageActions>
      <button
        type="button"
        className={controls.secondary}
        onClick={() => navigate("/providers")}
        style={{ display: "inline-flex", alignItems: "center", gap: "0.4rem", padding: "0.35rem 0.75rem", fontSize: "0.85rem" }}
      >
        {t("cursor.manage_providers")}
      </button>
    </PageActions>
    <PageContent title={t("models.models")} sections={[{ key: "cursor-settings", estimatedHeight: estimatedModelHeight, content }]} />
    <Modal fullHeight open={draft !== null} title={editing ? t("models.edit_model") : t("models.add_model")} banner={draft && (editorTesting || editorTestState) ? <CursorModelTestResult state={editorTestState} testing={editorTesting} /> : undefined} busy={cursorBusy || savingAndTesting} onClose={() => { if (editing && editorTesting) void cancelModelTest(editing.model_hash); setDraft(null); setEditing(null); }} onSubmit={() => void save()} submitLabel={t("common.save")} secondaryAction={<button type="button" className={controls.secondary} disabled={cursorBusy || savingAndTesting} onClick={() => void (editorTesting && editing ? cancelModelTest(editing.model_hash) : saveAndTest())}>{savingAndTesting ? t("common.processing") : editorTesting ? t("models.cancel_test") : t("models.save_and_test")}</button>}>
      {draft && <>
        <CursorModelEditor draft={draft} modelOptions={modelOptions} discovering={discovering} onChange={setDraft} onDiscover={discover} isEditing={Boolean(editing)} />
      </>}
    </Modal>
    <Modal open={settingsGroup !== null} title={t("models.group_settings")} busy={groupSettingsBusy || cursorBusy} onClose={() => setSettingsGroup(null)} onSubmit={() => void saveGroupSettings()} submitLabel={t("common.save")}>
      {settingsGroup && <div className={styles.editor}>
        <FormField label={t("models.group_name")} hint={t("models.applies_to_every_model_in_this_g")}>
          <TextInput placeholder={settingsGroup.key} value={groupNameDraft} onChange={(event) => setGroupNameDraft(event.target.value)} />
        </FormField>
        <FormField label={t("models.server_address")} hint={t("models.applies_to_every_model_in_this_g_2")}>
          <TextInput placeholder={t("models.leave_blank_to_keep_unchanged")} value={groupBaseUrlDraft} onChange={(event) => setGroupBaseUrlDraft(event.target.value)} />
        </FormField>
        <FormField label="API Key" hint={t("models.applies_to_every_model_in_this_g_2")}>
          <SecretTextInput placeholder={t("models.leave_blank_to_keep_unchanged")} autoComplete="off" value={groupApiKeyDraft} onChange={(event) => setGroupApiKeyDraft(event.target.value)} />
        </FormField>
      </div>}
    </Modal>
    <ConfirmDialog open={deleting !== null} title={t("models.delete_model")} cancelLabel={t("common.cancel")} confirmLabel={t("common.delete")} onCancel={() => setDeleting(null)} onConfirm={() => { if (deleting) void appStore.deleteModel(deleting.model_hash); setDeleting(null); }}><p>{t("models.delete_this_model")}</p></ConfirmDialog>
  </>;
}

function modelInput(model: Model): ModelInput {
  const { model_hash: _hash, created_at_ms: _created, updated_at_ms: _updated, ...input } = model;
  return input;
}

/** 组内所有模型取值一致时返回该值,否则返回 null(表单留空表示保持不变)。 */
function sharedValue(values: string[]): string | null {
  const [first, ...rest] = values;
  if (first === undefined) return null;
  return rest.every((value) => value === first) ? first : null;
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
  if (!model.display_name || !model.base_url || !model.api_key || !model.model_id) throw new Error(t("models.server_address_or_complete_reque"));
  for (const [label, value] of [[t("models.context_window_tokens"), model.context_window_tokens], [t("models.maximum_output_tokens"), model.type === "openai" ? model.max_completion_tokens : model.anthropic_max_tokens], [t("models.thinking_budget_tokens"), model.thinking_budget_tokens]] as const) {
    if (value !== null && (!Number.isSafeInteger(value) || value <= 0)) throw new Error(t("models.must_be_an_integer_greater_than_", { label }));
  }
  return model;
}

function parseHeaders(text: string): Record<string, string> {
  const parsed = parseObject(text, t("models.custom_headers"));
  if (Object.values(parsed).some((value) => typeof value !== "string")) throw new Error(t("models.all_custom_header_values_must_be"));
  return parsed as Record<string, string>;
}

function parseObject(text: string, label: string): Record<string, unknown> {
  let parsed: unknown;
  try { parsed = JSON.parse(text || "{}"); } catch { throw new Error(t("models.must_be_valid_json", { label })); }
  if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") throw new Error(t("models.must_be_a_json_object", { label }));
  return parsed as Record<string, unknown>;
}

