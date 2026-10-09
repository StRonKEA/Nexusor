import { useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  pluginText,
  type PluginAddMethod,
  type PluginDescriptor,
  type PluginOAuthBegin,
  type PluginProviderDescriptor,
  type PluginResourceAction,
  type PluginResourceActionResult,
  type PluginResourceDescriptor,
  type PluginResourceView,
} from "../../shared/api";
import { useI18n } from "../../i18n/store";
import { appStore, useAppStore } from "../../shared/store/appStore";
import { Button } from "../../shared/ui/Button";
import { Card } from "../../shared/ui/Card";
import { FormField, TextInput } from "../../shared/ui/FormControls";
import { Modal } from "../../shared/ui/Modal";
import { ConfirmDialog } from "../../shared/ui/ConfirmDialog";
import { Switch } from "../../shared/ui/Switch";
import { useMessage } from "../../shared/ui/message";
import { Icon } from "../../shared/ui/Icon";
import { editIcon } from "../../shared/ui/icons";
import { getProviderLogo } from "../../shared/utils/providerIcons";
import styles from "./PluginResourcePanels.module.scss";
import { errorText } from "../../shared/utils/errorText";

const PAGE_SIZE = 10;

export function PluginAddPanel({ plugin, onConfigured }: { plugin: PluginDescriptor; onConfigured: () => void }) {
  return <div className={styles.panel}>
    {plugin.resources.map((resource) => <ResourceAddSection
      key={resource.type}
      plugin={plugin}
      resource={resource}
      onConfigured={onConfigured}
    />)}
    {plugin.resources.length === 0 && <span className={styles.empty}>{t("plugins.this_plugin_does_not_need_any_re")}</span>}
  </div>;
}

function ResourceAddSection({ plugin, resource, onConfigured }: {
  plugin: PluginDescriptor;
  resource: PluginResourceDescriptor;
  onConfigured: () => void;
}) {
  return <>
    {resource.add.map((method) => {
      if (method.type === "api_key") {
        return <ApiKeyMethodCard
          key={method.id}
          pluginId={plugin.id}
          resourceType={resource.type}
          method={method}
          onConfigured={onConfigured}
        />;
      }
      return <OAuthMethodCard
        key={method.id}
        pluginId={plugin.id}
        resourceType={resource.type}
        method={method}
        onConfigured={onConfigured}
      />;
    })}
  </>;
}

function ApiKeyMethodCard({
  pluginId,
  resourceType,
  method,
  onConfigured,
}: {
  pluginId: string;
  resourceType: string;
  method: PluginAddMethod;
  onConfigured: () => void;
}) {
  const { locale } = useI18n();
  const [apiKey, setApiKey] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);

  const isNvidia = pluginId.includes("nvidia");
  const isGroq = pluginId.includes("groq");
  const isOpencode = pluginId.includes("opencode");
  const logo = getProviderLogo(pluginId);

  const getKeyUrl = isNvidia
    ? "https://build.nvidia.com/"
    : isGroq
    ? "https://console.groq.com/keys"
    : "https://opencode.ai/auth";

  const handleConnect = async () => {
    if (!apiKey.trim()) return;
    setSaving(true);
    setError(null);
    try {
      const keySuffix = apiKey.trim().slice(-4);
      const namePrefix = pluginId.includes("nvidia")
        ? "NVIDIA"
        : pluginId.includes("groq")
        ? "Groq"
        : "OpenCode";

      const payload = {
        key: `${pluginId}:${Date.now()}`,
        privateData: {
          apiKey: apiKey.trim(),
          baseUrl: baseUrl.trim() || undefined,
          displayName: `${namePrefix} (...${keySuffix})`,
        },
      };

      await api.importPluginResources(pluginId, resourceType, [
        {
          name: "credentials.json",
          content: JSON.stringify(payload),
        },
      ]);

      const providerId = pluginId.includes("nvidia")
        ? "nvidia-nim"
        : pluginId.includes("groq")
        ? "groq"
        : "opencode";

      await api.syncPluginModels(pluginId, providerId);
      await appStore.refreshPlugins();

      setSuccess(true);
      onConfigured();
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card className={styles.methodCard}>
      <div className={styles.methodHeader}>
        <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
          {logo && (
            <img
              src={logo}
              alt=""
              style={{ width: "32px", height: "32px", objectFit: "contain", borderRadius: "6px" }}
            />
          )}
          <div style={{ display: "flex", flexDirection: "column", gap: "2px" }}>
            <strong>{pluginText(method.displayName, locale)}</strong>
            {method.description && <small>{pluginText(method.description, locale)}</small>}
          </div>
        </div>
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: "10px", margin: "12px 0 6px" }}>
        <FormField
          label={t("plugins.api_key_field_label")}
          hint={t("plugins.api_key_field_hint")}
        >
          <div style={{ display: "flex", gap: "8px", alignItems: "center" }}>
            <TextInput
              type="password"
              placeholder={
                pluginId.includes("nvidia")
                  ? "nvapi-..."
                  : pluginId.includes("groq")
                  ? "gsk_..."
                  : "sk-..."
              }
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
            />
            <Button
              size="small"
              variant="secondary"
              onClick={() => void api.openExternalUrl(getKeyUrl)}
            >
              {getKeyUrl.replace("https://", "").replace(/\/$/, "")} ↗
            </Button>
          </div>
        </FormField>

        {isOpencode && (
          <FormField
            label={t("plugins.base_url_field_label")}
            hint={t("plugins.base_url_field_hint")}
          >
            <TextInput
              placeholder="https://opencode.ai/zen/v1"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
            />
          </FormField>
        )}
      </div>

      <div className={styles.actions}>
        <Button
          variant="primary"
          disabled={saving || !apiKey.trim()}
          onClick={() => void handleConnect()}
        >
          {saving ? t("settings.saving") : t("plugins.connect_api_key_button")}
        </Button>
      </div>

      {success && (
        <span className={styles.success}>{t("plugins.account_saved_and_the_model_cata")}</span>
      )}
      {error && (
        <span className={styles.error} role="alert">
          {error}
        </span>
      )}
    </Card>
  );
}

function OAuthMethodCard({ pluginId, resourceType, method, onConfigured }: {
  pluginId: string;
  resourceType: string;
  method: PluginAddMethod;
  onConfigured: () => void;
}) {
  const { locale } = useI18n();
  const [status, setStatus] = useState<"idle" | "starting" | "polling" | "success" | "error">("idle");
  const [begun, setBegun] = useState<PluginOAuthBegin | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const stopped = useRef(false);

  const copyCode = async (code: string) => {
    await api.copyCursorText(code).catch(() => undefined);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 2000);
  };

  useEffect(() => () => { stopped.current = true; }, []);

  useEffect(() => {
    if (!begun || status !== "polling") return;
    let timer = 0;
    const poll = async (intervalMs: number) => {
      if (stopped.current) return;
      try {
        const result = await api.pluginOAuthPoll(begun.sessionId);
        if (stopped.current) return;
        if (result.status === "pending" || result.status === "slow-down") {
          const base = Number(result.pollIntervalMs) > 0
            ? result.pollIntervalMs
            : intervalMs > 0
              ? intervalMs
              : begun.pollIntervalMs || 5_000;
          const next = result.status === "slow-down"
            ? Math.max(base * 2, 5_000)
            : Math.max(base, 1_000);
          timer = window.setTimeout(() => void poll(next), next);
          return;
        }
        if (result.status === "completed") {
          await appStore.refreshPlugins();
          if (result.modelSyncError) {
            setStatus("error");
            setError(t("plugins.the_account_was_saved_but_model_", { error: result.modelSyncError }));
            return;
          }
          setStatus("success");
          onConfigured();
          return;
        }
        setStatus("error");
        setError(result.message || t("plugins.authorization_was_denied_or_fail"));
      } catch (cause) {
        if (stopped.current) return;
        setError(errorText(cause));
        timer = window.setTimeout(() => void poll(intervalMs), Math.max(1000, intervalMs || 5_000));
      }
    };
    timer = window.setTimeout(
      () => void poll(begun.pollIntervalMs || 5_000),
      Math.max(1000, begun.pollIntervalMs || 5_000),
    );
    return () => window.clearTimeout(timer);
  }, [begun, onConfigured, status]);

  const start = async () => {
    setStatus("starting");
    setError(null);
    try {
      const next = await api.pluginOAuthBegin(pluginId, resourceType, method.id);
      setBegun(next);
      setStatus("polling");
      if (next.userCode) await api.copyCursorText(next.userCode).catch(() => undefined);
      if (next.verificationUrlComplete || next.verificationUrl) {
        void api.openExternalUrl(next.verificationUrlComplete || next.verificationUrl);
      }
    } catch (cause) {
      setStatus("error");
      setError(errorText(cause));
    }
  };

  const logo = getProviderLogo(pluginId);

  return <Card className={styles.methodCard}>
    <div className={styles.methodHeader}>
      <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
        {logo && <img src={logo} alt="" style={{ width: "32px", height: "32px", objectFit: "contain", borderRadius: "6px" }} />}
        <div style={{ display: "flex", flexDirection: "column", gap: "2px" }}>
          <strong>{pluginText(method.displayName, locale)}</strong>
          {method.description && <small>{pluginText(method.description, locale)}</small>}
        </div>
      </div>
    </div>
    {begun?.userCode && <div className={styles.codeBlock}>
      <small>{t("plugins.device_code")}</small>
      <div className={styles.codeRow}>
        <code>{begun.userCode}</code>
        <Button size="small" onClick={() => void copyCode(begun.userCode!)}>
          {copied ? t("plugins.copied") : t("common.copy")}
        </Button>
      </div>
    </div>}
    <div className={styles.actions}>
      <Button variant="primary" disabled={status === "starting" || status === "polling"} onClick={() => void start()}>
        {status === "starting" ? t("plugins.requesting_an_authorization_code") : status === "polling" ? t("plugins.waiting_for_browser_authorizatio") : t("plugins.start_sign_in")}
      </Button>
      {begun && status === "polling" && <Button onClick={() => void api.openExternalUrl(begun.verificationUrlComplete || begun.verificationUrl)}>{t("plugins.open_authorization_page")}</Button>}
    </div>
    {status === "success" && <span className={styles.success}>{t("plugins.account_saved_and_the_model_cata")}</span>}
    {error && <span className={styles.error} role="alert">{error}</span>}
  </Card>;
}

export function PluginSettingsPanel({ plugin }: { plugin: PluginDescriptor }) {
  const { locale } = useI18n();
  const message = useMessage();
  const { plugins } = useAppStore();
  const [managingProviderId, setManagingProviderId] = useState<string | null>(null);
  const [syncing, setSyncing] = useState<Record<string, boolean>>({});
  const [busy, setBusy] = useState(false);
  const [deletingResource, setDeletingResource] = useState<{ resourceType: string; item: PluginResourceView } | null>(null);
  const livePlugin = plugins.find((entry) => entry.id === plugin.id) ?? plugin;
  const managingProvider = managingProviderId
    ? livePlugin.providers.find((provider) => provider.id === managingProviderId) ?? null
    : null;

  const sync = async (providerId: string) => {
    setSyncing((current) => ({ ...current, [providerId]: true }));
    try {
      const result = await api.syncPluginModels(plugin.id, providerId);
      await appStore.refreshPlugins();
      message(t("plugins.synced_models", { count: result.models }));
    } catch (cause) {
      message(errorText(cause), { duration: 5_000 });
    } finally {
      setSyncing((current) => ({ ...current, [providerId]: false }));
    }
  };

  const removeResource = async (resourceType: string, resourceId: string) => {
    setBusy(true);
    try {
      await api.deletePluginResource(plugin.id, resourceType, resourceId);
      await appStore.refreshPlugins();
    } finally {
      setBusy(false);
    }
  };

  const refreshResource = async (resourceType: string, resourceId: string) => {
    setBusy(true);
    try {
      await api.refreshPluginResource(plugin.id, resourceType, resourceId);
      await appStore.refreshPlugins();
    } finally {
      setBusy(false);
    }
  };

  return <div className={styles.panel}>
    <FormField label={t("plugins.providers_and_models")}>
      <div className={styles.providerList}>
        {livePlugin.providers.map((provider) => <ProviderRow
          key={provider.id}
          provider={provider}
          busy={busy}
          syncing={Boolean(syncing[provider.id])}
          canSync={Boolean(
            provider.resourceType
              ? livePlugin.resources.some((resource) =>
                  resource.type === provider.resourceType && resource.resources.length > 0)
              : true,
          )}
          onManageModels={() => setManagingProviderId(provider.id)}
          onSync={() => void sync(provider.id)}
        />)}
      </div>
    </FormField>
    {livePlugin.resources.map((resource) => <ResourceListSection
      key={resource.type}
      pluginId={plugin.id}
      resource={resource}
      busy={busy}
      onRefresh={(item) => void refreshResource(resource.type, item.id)}
      onDelete={(item) => setDeletingResource({ resourceType: resource.type, item })}
    />)}
    <Modal
      fullHeight
      open={managingProvider !== null}
      title={t("plugins.model_management", { name: managingProvider ? pluginText(managingProvider.displayName, locale) : "" })}
      onClose={() => setManagingProviderId(null)}
      onSubmit={() => setManagingProviderId(null)}
      submitLabel={t("common.confirm")}
    >
      {managingProvider && <ModelManagementDialog
        pluginId={plugin.id}
        provider={managingProvider}
        busy={busy}
      />}
    </Modal>
    <ConfirmDialog
      open={deletingResource !== null}
      title={t("plugins.delete_account_confirm_title")}
      cancelLabel={t("common.cancel")}
      confirmLabel={t("common.delete")}
      onCancel={() => setDeletingResource(null)}
      onConfirm={() => {
        if (deletingResource) {
          void removeResource(deletingResource.resourceType, deletingResource.item.id);
          setDeletingResource(null);
        }
      }}
    >
      <p>
        {t("plugins.delete_account_confirm_desc", { name: deletingResource?.item.displayName || "" })}
      </p>
    </ConfirmDialog>
  </div>;
}

function ProviderRow({ provider, busy, syncing, canSync, onManageModels, onSync }: {
  provider: PluginProviderDescriptor;
  busy: boolean;
  syncing: boolean;
  canSync: boolean;
  onManageModels: () => void;
  onSync: () => void;
}) {
  const { locale } = useI18n();
  const logo = getProviderLogo(provider.id) || getProviderLogo(provider.displayName ? pluginText(provider.displayName, locale) : "");
  return <Card className={styles.providerRow}>
    <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
      {logo && <img src={logo} alt="" style={{ width: "24px", height: "24px", objectFit: "contain", borderRadius: "4px" }} />}
      <div className={styles.providerInfo}>
        <strong>{pluginText(provider.displayName, locale)}</strong>
        <span>
          {provider.models.length > 0 ? t("plugins.models", { count: provider.models.length }) : t("plugins.models_not_synced_yet")}
          {" · "}
          {provider.configured ? t("plugins.callable") : t("cursor.not_ready")}
        </span>
      </div>
    </div>
    <div className={styles.actions}>
      <Button size="small" disabled={busy || provider.models.length === 0} onClick={onManageModels}>{t("plugins.model_management_2")}</Button>
      {provider.hasModels && <Button size="small" disabled={busy || syncing || !canSync} onClick={onSync}>
        {syncing ? t("plugins.syncing") : t("plugins.sync_models")}
      </Button>}
    </div>
  </Card>;
}

function ModelManagementDialog({ pluginId, provider, busy }: {
  pluginId: string;
  provider: PluginProviderDescriptor;
  busy: boolean;
}) {
  const toggle = async (modelId: string, enabled: boolean) => {
    await api.setPluginModelEnabled(pluginId, provider.id, modelId, enabled);
    await appStore.refreshPlugins();
  };

  return <div className={styles.modelDialog}>
    <table className={styles.modelTable}>
      <thead><tr><th scope="col">{t("calls.model_name")}</th><th scope="col">{t("combos.enabled")}</th></tr></thead>
      <tbody>
        {provider.models.map((model) => <tr key={model.id}>
          <td>
            <strong>{model.displayName}</strong>
            <code>{model.modelId}</code>
          </td>
          <td>
            <Switch
              checked={model.enabled}
              disabled={busy}
              label={t("plugins.enable", { model: model.displayName })}
              onChange={(enabled) => void toggle(model.modelId, enabled)}
            />
          </td>
        </tr>)}
      </tbody>
    </table>
    {provider.models.length === 0 && <span className={styles.empty}>{t("plugins.models_not_synced_yet")}</span>}
  </div>;
}

function ResourceListSection({ pluginId, resource, busy, onRefresh, onDelete }: {
  pluginId: string;
  resource: PluginResourceDescriptor;
  busy: boolean;
  onRefresh: (item: PluginResourceView) => void;
  onDelete: (item: PluginResourceView) => void;
}) {
  const { locale } = useI18n();
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(1);

  const filtered = useMemo(() => {
    const term = query.trim().toLowerCase();
    if (!term) return resource.resources;
    return resource.resources.filter((item) =>
      item.displayName.toLowerCase().includes(term) ||
      item.id.toLowerCase().includes(term)
    );
  }, [query, resource.resources]);

  const pageCount = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
  const visible = filtered.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE);

  useEffect(() => setPage(1), [query]);

  return <FormField label={pluginText(resource.displayName, locale)}>
    <div className={styles.resourceSection}>
      {resource.resources.length > PAGE_SIZE && <div className={styles.toolbar}>
        <TextInput aria-label={t("plugins.search_resources")} placeholder={t("plugins.search_resources")} value={query} onChange={(event) => setQuery(event.target.value)} />
      </div>}
      <div className={styles.resourceList}>
        {visible.map((item) => <ResourceRow
          key={item.id}
          pluginId={pluginId}
          resourceType={resource.type}
          item={item}
          actions={resource.actions}
          canRefresh={resource.canRefresh}
          disabled={busy}
          onRefresh={() => onRefresh(item)}
          onDelete={() => onDelete(item)}
        />)}
        {visible.length === 0 && <span className={styles.empty}>{t("plugins.no_resources_yet_add_one_first")}</span>}
      </div>
      {pageCount > 1 && <div className={styles.pagination}>
        <Button size="small" disabled={page <= 1} onClick={() => setPage((current) => current - 1)}>{t("common.previous_page")}</Button>
        <span>{t("plugins.page", { page: Math.min(page, pageCount), total: pageCount })}</span>
        <Button size="small" disabled={page >= pageCount} onClick={() => setPage((current) => current + 1)}>{t("common.next_page")}</Button>
      </div>}
    </div>
  </FormField>;
}

function ResourceRow({ pluginId, resourceType, item, actions, canRefresh, disabled, onRefresh, onDelete }: {
  pluginId: string;
  resourceType: string;
  item: PluginResourceView;
  actions: PluginResourceAction[];
  canRefresh: boolean;
  disabled: boolean;
  onRefresh: () => void;
  onDelete: () => void;
}) {
  const { locale } = useI18n();
  const message = useMessage();
  const [actionBusy, setActionBusy] = useState(false);
  const [actionResult, setActionResult] = useState<PluginResourceActionResult | null>(null);
  const [editingNote, setEditingNote] = useState(false);
  const [noteDraft, setNoteDraft] = useState(item.note || "");

  useEffect(() => {
    setNoteDraft(item.note || "");
  }, [item.note]);

  const saveNote = async () => {
    setActionBusy(true);
    try {
      await api.pluginResourceAction(pluginId, resourceType, item.id, "update-note", {
        note: noteDraft.trim(),
      });
      setEditingNote(false);
      await appStore.refreshPlugins();
      message(t("plugins.note_saved"));
    } catch (cause) {
      message(errorText(cause));
    } finally {
      setActionBusy(false);
    }
  };
  const isQuotaExhausted = item.state.status === "cooling" && (
    Boolean(item.state.message?.toLowerCase().includes("quota")) ||
    Boolean(item.state.message?.toLowerCase().includes("limit")) ||
    Boolean(item.state.message?.toLowerCase().includes("capacity")) ||
    Boolean(item.state.message?.toLowerCase().includes("exhausted"))
  );
  const statusLabel = isQuotaExhausted
    ? "Kota Doldu"
    : item.state.status === "cooling"
      ? t("plugins.cooling_down")
      : item.state.status === "invalid"
        ? t("plugins.invalid")
        : t("cursor.ready");
  const retryLabel = item.state.retryAtMs
    ? `${t("plugins.reset")}: ${formatResetTime(item.state.retryAtMs, locale)}`
    : null;

  const toggleResourceEnabled = async (enabled: boolean) => {
    setActionBusy(true);
    try {
      await api.setPluginResourceEnabled(pluginId, resourceType, item.id, enabled);
      await appStore.refreshPlugins();
      message(enabled ? "Hesap aktif duruma alındı" : "Hesap pasif duruma alındı");
    } catch (cause) {
      message(errorText(cause));
    } finally {
      setActionBusy(false);
    }
  };

  const runAction = async (action: PluginResourceAction) => {
    setActionBusy(true);
    try {
      const result = await api.pluginResourceAction(pluginId, resourceType, item.id, action.id);
      setActionResult(result);
      await appStore.refreshPlugins();
    } catch (cause) {
      setActionResult({
        title: t("plugins.action_failed"),
        description: errorText(cause),
        cards: [],
      });
    } finally {
      setActionBusy(false);
    }
  };

  return <>
    <Card className={styles.resourceRow}>
      <div className={styles.resourceInfo}>
        <div className={styles.compactHeaderRow}>
          <div className={styles.accountInfoBadge}>
            <span className={styles.accountEmail}>{item.displayName}</span>
            {item.description && (
              <span className={styles.accountPlanBadge}>
                {pluginText(item.description, locale)}
              </span>
            )}
            {item.note && (
              <span
                className={styles.accountNoteBadge}
                title={t("plugins.edit_account_note")}
                onClick={() => setEditingNote(true)}
              >
                {item.note}
              </span>
            )}
            <button
              type="button"
              className={styles.editNoteBtn}
              title={t("plugins.edit_account_note")}
              onClick={() => setEditingNote(true)}
            >
              <Icon icon={editIcon} size="0.75em" />
            </button>
            <span
              className={`${styles.stateBadge} ${
                item.state.status === "disabled"
                  ? styles.stateDisabled
                  : item.state.status === "ready"
                  ? styles.stateReady
                  : item.state.status === "cooling"
                  ? styles.stateCooling
                  : styles.stateInvalid
              }`}
            >
              {item.state.status === "disabled" ? "Pasif" : statusLabel}
            </span>
          </div>

          <div className={styles.headerActions}>
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <span style={{ fontSize: 11, color: "#8a8f98" }}>
                {item.state.status === "disabled" ? "Pasif" : "Aktif"}
              </span>
              <Switch
                checked={item.state.status !== "disabled"}
                disabled={disabled || actionBusy}
                label={item.state.status === "disabled" ? "Pasif" : "Aktif"}
                onChange={(enabled) => void toggleResourceEnabled(enabled)}
              />
            </div>
            {canRefresh && (
              <Button
                size="small"
                variant="secondary"
                disabled={disabled || actionBusy}
                onClick={onRefresh}
              >
                {t("common.refresh")}
              </Button>
            )}
            <Button
              size="small"
              variant="secondary"
              disabled={disabled || actionBusy}
              onClick={onDelete}
            >
              {t("common.delete")}
            </Button>
            {actions.filter((action) => action.target === "resource").map((action) => {
              const isResetCreditAction = action.id === "redeem-reset-credit";
              const resetCreditMetric = item.metrics.find((m) => m.id === "reset-credits");
              const creditCount = Math.round(resetCreditMetric?.value ?? 0);
              const actionDisabled = disabled || actionBusy || (isResetCreditAction && creditCount <= 0);

              return (
                <Button
                  key={action.id}
                  size="small"
                  variant={isResetCreditAction && creditCount > 0 ? "primary" : "secondary"}
                  disabled={actionDisabled}
                  title={
                    isResetCreditAction && creditCount <= 0
                      ? t("plugins.reset_credit_no_tokens_title")
                      : undefined
                  }
                  onClick={() => void runAction(action)}
                >
                  {pluginText(action.displayName, locale)}
                </Button>
              );
            })}
          </div>
        </div>

        {item.state.message && <span className={styles.resourceSub}>{item.state.message}</span>}
        {retryLabel && <span className={styles.resourceSub}>{retryLabel}</span>}
        <div className={styles.metricTags}>
          {item.metrics.map((m) => {
            let label = pluginText(m.label, locale);
            let fillColor = "#3186FF";
            let fullName = label;
            let fullPeriod = "";

            if (m.id === "claude-quota") {
              label = "Claude (5 Saat)";
              fullName = "Claude";
              fullPeriod = "5 Saatlik Oturum Kotası";
              fillColor = m.value > 20 ? "#D97757" : "#ef4444";
            } else if (m.id === "claude-weekly-quota") {
              label = "Claude (Hafta)";
              fullName = "Claude";
              fullPeriod = "Haftalık Toplam Kota";
              fillColor = m.value > 20 ? "#D97757" : "#ef4444";
            } else if (m.id === "gemini-quota") {
              label = "Gemini (5 Saat)";
              fullName = "Gemini";
              fullPeriod = "5 Saatlik Oturum Kotası";
              fillColor = m.value > 20 ? "#3186FF" : "#ef4444";
            } else if (m.id === "gemini-weekly-quota") {
              label = "Gemini (Hafta)";
              fullName = "Gemini";
              fullPeriod = "Haftalık Toplam Kota";
              fillColor = m.value > 20 ? "#3186FF" : "#ef4444";
            } else if (m.id === "primary-quota") {
              label = "Codex (5 Saat)";
              fullName = "OpenAI Codex";
              fullPeriod = "5 Saatlik Oturum Kotası";
              fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
            } else if (m.id === "secondary-quota") {
              label = "Codex (Hafta)";
              fullName = "OpenAI Codex";
              fullPeriod = "Haftalık Toplam Kota";
              fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
            } else if (m.id === "copilot-status") {
              label = "Copilot";
              fullName = "GitHub Copilot";
              fullPeriod = "Aktif Abonelik";
              fillColor = "#238636";
            } else if (m.id === "credits") {
              label = "Grok (Hafta)";
              fullName = "xAI Grok";
              fullPeriod = "Haftalık Kredi";
              fillColor = m.value > 20 ? "#e0e0e0" : "#ef4444";
            } else if (m.id === "kimi-5h") {
              label = "Kimi (5 Saat)";
              fullName = "Moonshot Kimi";
              fullPeriod = "5 Saatlik Oturum Kotası";
              fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
            } else if (m.id === "kimi-weekly") {
              label = "Kimi (Hafta)";
              fullName = "Moonshot Kimi";
              fullPeriod = "Haftalık Toplam Kota";
              fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
            } else if (m.id === "claude-code-5h") {
              label = "Claude (5 Saat)";
              fullName = "Claude Code";
              fullPeriod = "5 Saatlik Oturum Kotası";
              fillColor = m.value > 20 ? "#D97757" : "#ef4444";
            } else if (m.id === "claude-code-weekly") {
              label = "Claude (Hafta)";
              fullName = "Claude Code";
              fullPeriod = "Haftalık Toplam Kota";
              fillColor = m.value > 20 ? "#D97757" : "#ef4444";
            } else if (m.id === "claude-code-sonnet") {
              label = "Sonnet (Hafta)";
              fullName = "Claude Code Sonnet";
              fullPeriod = "Haftalık Özel Kota";
              fillColor = m.value > 20 ? "#D97757" : "#ef4444";
            } else if (m.id === "nvidia-status") {
              label = "NVIDIA NIM";
              fullName = "NVIDIA NIM";
              fullPeriod = "NVIDIA NIM";
              fillColor = "#74B71B";
            } else if (m.id === "opencode-status") {
              label = "OpenCode";
              fullName = "OpenCode";
              fullPeriod = "OpenCode API";
              fillColor = "#ffffff";
            } else if (m.id === "groq-status") {
              label = "Groq LPU";
              fullName = "Groq";
              fullPeriod = "Groq LPU";
              fillColor = "#f55036";
            }

            if (m.id === "reset-credits") {
              const creditCount = Math.round(m.value);
              const hasCredits = creditCount > 0;
              return (
                <div
                  key={m.id}
                  className={styles.quotaItem}
                  style={{
                    background: hasCredits ? "rgba(16, 163, 127, 0.12)" : "rgba(255, 255, 255, 0.03)",
                    borderColor: hasCredits ? "rgba(16, 163, 127, 0.3)" : "rgba(255, 255, 255, 0.08)",
                  }}
                  title={
                    hasCredits
                      ? t("plugins.reset_credit_tooltip_has", { count: creditCount })
                      : t("plugins.reset_credit_tooltip_none")
                  }
                >
                  <span className={styles.quotaLabel}>{t("plugins.reset_credit_label")}:</span>
                  <span
                    style={{
                      fontWeight: 700,
                      color: hasCredits ? "#10a37f" : "#8a8f98",
                      fontVariantNumeric: "tabular-nums",
                    }}
                  >
                    {hasCredits
                      ? t("plugins.reset_credit_ready", { count: creditCount })
                      : t("plugins.reset_credit_none")}
                  </span>
                </div>
              );
            }

            const percent = Math.round(Math.max(0, Math.min(100, m.value)));
            let tooltip = m.id === "copilot-status"
              ? "GitHub Copilot: Aktif Abonelik (Sınırsız Kullanım)"
              : m.id === "nvidia-status"
              ? "NVIDIA NIM: Aktif"
              : m.id === "opencode-status"
              ? "OpenCode: Aktif"
              : m.id === "groq-status"
              ? "Groq: Aktif"
              : fullPeriod
                ? `${fullName} (${fullPeriod}): %${percent} kalan`
                : `${fullName}: %${percent}`;

            if (m.unit === "status") {
              return (
                <div key={m.id} className={styles.quotaItem} title={tooltip}>
                  <span className={styles.quotaLabel}>{label}:</span>
                  <span style={{ fontWeight: 600, color: fillColor }}>
                    {t("cursor.ready")}
                  </span>
                </div>
              );
            }

            if (m.resetAtMs) {
              const date = new Date(m.resetAtMs);
              tooltip += ` · Sıfırlanma: ${date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
            }

            return (
              <div key={m.id} className={styles.quotaItem} title={tooltip}>
                <span className={styles.quotaLabel}>{label}:</span>
                <div className={styles.quotaTrack}>
                  <div
                    className={styles.quotaFill}
                    style={{ width: `${percent}%`, backgroundColor: fillColor }}
                  />
                </div>
                <span className={styles.quotaValue}>
                  {m.unit === "count" ? Math.round(m.value) : `%${percent}`}
                </span>
                {m.resetAtMs ? (
                  <span className={styles.quotaReset}>
                    · {formatResetTime(m.resetAtMs, locale)}
                  </span>
                ) : null}
              </div>
            );
          })}
        </div>
      </div>
    </Card>
    <Modal
      open={actionResult !== null}
      title={actionResult ? pluginText(actionResult.title, locale) : ""}
      onClose={() => setActionResult(null)}
      onSubmit={() => setActionResult(null)}
      submitLabel={t("common.confirm")}
    >
      {actionResult && <div className={styles.actionResult}>
        {actionResult.description && <p>{pluginText(actionResult.description, locale)}</p>}
        {actionResult.cards.map((card) => <Card key={card.id} className={styles.actionCard}>
          <strong>{pluginText(card.title, locale)}</strong>
          {card.status && <span>{pluginText(card.status, locale)}</span>}
          {card.fields.map((field) => <div key={field.id} className={styles.actionField}>
            <small>{pluginText(field.label, locale)}</small>
            <code>{field.value}</code>
          </div>)}
        </Card>)}
      </div>}
    </Modal>
    <Modal
      open={editingNote}
      title={t("plugins.edit_account_note_title")}
      onClose={() => setEditingNote(false)}
      onSubmit={() => void saveNote()}
      submitLabel={t("common.save")}
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 12, padding: "8px 0" }}>
        <FormField
          label={t("plugins.account_nickname_label")}
          hint={t("plugins.account_nickname_hint")}
        >
          <TextInput
            value={noteDraft}
            placeholder={t("plugins.account_nickname_placeholder")}
            onChange={(e) => setNoteDraft(e.target.value)}
          />
        </FormField>
      </div>
    </Modal>
  </>;
}

function formatResetTime(ms: number, locale: string): string {
  const targetMs = Number(ms);
  if (!Number.isFinite(targetMs) || targetMs <= 0) return "";
  const now = Date.now();
  const diffMs = targetMs - now;
  const date = new Date(targetMs);
  const timeStr = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

  const rtf = new Intl.RelativeTimeFormat(locale || "en", { numeric: "auto" });

  if (diffMs <= 0) {
    return timeStr;
  }

  const minutes = Math.ceil(diffMs / 60_000);
  if (minutes < 60) {
    return `${rtf.format(minutes, "minute")} (${timeStr})`;
  }

  const hours = Math.round(minutes / 60);
  if (hours < 24) {
    return `${rtf.format(hours, "hour")} (${timeStr})`;
  }

  const days = Math.round(hours / 24);
  return `${rtf.format(days, "day")} (${date.toLocaleDateString(locale, { month: "short", day: "numeric" })} ${timeStr})`;
}

