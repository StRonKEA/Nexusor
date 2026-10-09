import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { api, type ProxySettings, type ProxySettingsInput, type StatisticsStorage, type StatisticsStorageScope } from "../../shared/api";
import { PageContent } from "../../shell/layout/PageContent";
import { BackupSettingsCard } from "./BackupSettingsCard";
import { AppLifecycleSettingsCard } from "./AppLifecycleSettingsCard";
import { ProxySettingsCard } from "./ProxySettingsCard";
import { Button } from "../../shared/ui/Button";
import { Checkbox } from "../../shared/ui/Checkbox";
import { ConfirmDialog } from "../../shared/ui/ConfirmDialog";
import { FormField, TextInput } from "../../shared/ui/FormControls";
import { Select } from "../../shared/ui/Select";
import { TitledCard } from "../../shared/ui/TitledCard";
import { setLocalePreference, useI18n, type LocalePreference } from "../../i18n/store";
import { useMessage } from "../../shared/ui/message";
import { appStore, useAppStore } from "../../shared/store/appStore";

import styles from "./SettingsPage.module.scss";

type SettingsTab = "app" | "connection" | "observability" | "import";

export function SettingsPage() {
  const { detailed, ports } = useAppStore();
  const { preference, locale } = useI18n();
  const message = useMessage();
  const [searchParams, setSearchParams] = useSearchParams();
  const tabs: { id: SettingsTab; label: string }[] = [
    { id: "app", label: t("settings.application") },
    { id: "connection", label: t("settings.connection") },
    { id: "observability", label: t("settings.call_observability") },
    { id: "import", label: t("settings.import_export") },
  ];
  const tabParam = searchParams.get("tab");
  const activeTab: SettingsTab = tabs.some((tab) => tab.id === tabParam)
    ? tabParam as SettingsTab
    : "app";
  const [proxyPort, setProxyPort] = useState(String(ports.proxy_port));
  const [servicePort, setServicePort] = useState(String(ports.service_port));
  const [editingPorts, setEditingPorts] = useState(false);
  const [savingPorts, setSavingPorts] = useState(false);
  const [storage, setStorage] = useState<StatisticsStorage | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);
  const [clearScope, setClearScope] = useState<StatisticsStorageScope>("details");
  const [clearing, setClearing] = useState(false);
  const [outboundProxy, setOutboundProxy] = useState<ProxySettings | null>(null);
  const [proxyDraft, setProxyDraft] = useState<ProxySettingsInput>({ mode: "default", address: "", auth_enabled: false, username: "", password: "" });
  const [editingProxy, setEditingProxy] = useState(false);
  const [savingProxy, setSavingProxy] = useState(false);
  const [quotaAlertThreshold, setQuotaAlertThreshold] = useState(() => {
    return localStorage.getItem("nexusor_quota_alert_threshold") || "15";
  });

  useEffect(() => {
    const report = (cause: unknown) => message(cause instanceof Error ? cause.message : String(cause));
    void api.statisticsStorage().then(setStorage).catch(report);
    void api.proxySettings().then((next) => {
      setOutboundProxy(next);
      setProxyDraft({ mode: next.mode, address: next.address, auth_enabled: next.auth_enabled, username: next.username, password: "" });
    }).catch(report);
  }, [message]);
  useEffect(() => {
    setProxyPort(String(ports.proxy_port));
    setServicePort(String(ports.service_port));
  }, [ports.proxy_port, ports.service_port]);

  const parsePort = (value: string, label: string) => {
    const port = Number(value);
    if (!Number.isInteger(port) || port < 0 || port > 65_535) {
      throw new Error(`${label}${t("settings.must_be_an_integer_from_0_to_655")}`);
    }
    return port;
  };
  const savePorts = async () => {
    try {
      const next = {
        proxy_port: parsePort(proxyPort, t("settings.proxy_port")),
        service_port: parsePort(servicePort, t("settings.service_port")),
      };
      setSavingPorts(true);
      if (await appStore.updatePorts(next)) {
        setEditingPorts(false);
        message(t("settings.port_settings_saved_restart_the_"), { duration: 4_000 });
      }
    } catch (cause) {
      message(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSavingPorts(false);
    }
  };
  const editPorts = () => {
    setProxyPort(String(ports.proxy_port));
    setServicePort(String(ports.service_port));
    setEditingPorts(true);
  };
  const cancelPortEdit = () => {
    setProxyPort(String(ports.proxy_port));
    setServicePort(String(ports.service_port));
    setEditingPorts(false);
  };
  const clearStorage = async () => {
    try {
      setClearing(true);
      setStorage(await api.clearStatisticsStorage(clearScope));
      setConfirmClear(false);
      await appStore.refresh();
      message(clearScope === "all" ? t("settings.all_statistics_cleared") : t("settings.detailed_records_cleared"));
    } catch (cause) {
      message(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setClearing(false);
    }
  };
  const editProxy = () => {
    if (!outboundProxy) return;
    setProxyDraft({ mode: outboundProxy.mode, address: outboundProxy.address, auth_enabled: outboundProxy.auth_enabled, username: outboundProxy.username, password: "" });
    setEditingProxy(true);
  };
  const cancelProxyEdit = () => {
    if (outboundProxy) {
      setProxyDraft({ mode: outboundProxy.mode, address: outboundProxy.address, auth_enabled: outboundProxy.auth_enabled, username: outboundProxy.username, password: "" });
    }
    setEditingProxy(false);
  };
  const saveProxy = async () => {
    try {
      setSavingProxy(true);
      const saved = await api.setProxySettings({ ...proxyDraft, password: proxyDraft.password || undefined });
      setOutboundProxy(saved);
      setProxyDraft({ mode: saved.mode, address: saved.address, auth_enabled: saved.auth_enabled, username: saved.username, password: "" });
      setEditingProxy(false);
      message(t("settings.proxy_settings_saved"));
    } catch (cause) {
      message(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSavingProxy(false);
    }
  };
  const clearTitle = clearScope === "all" ? t("settings.clear_all_statistics") : t("settings.clear_detailed_records");
  const clearDescription = clearScope === "all"
    ? t("settings.all_call_summaries_detailed_cont")
    : t("settings.only_request_response_and_trace_");

  const setTab = (id: SettingsTab) => {
    setSearchParams(id === "app" ? {} : { tab: id }, { replace: true });
  };

  let body: ReactNode = null;
  if (activeTab === "connection") {
    body = <>
      <TitledCard title={t("settings.port_settings")} action={editingPorts ? (
        <div className={styles.cardActions}>
          <Button size="small" disabled={savingPorts} onClick={cancelPortEdit}>{t("common.cancel")}</Button>
          <Button variant="primary" size="small" disabled={savingPorts} onClick={() => void savePorts()}>{savingPorts ? t("settings.saving") : t("common.save")}</Button>
        </div>
      ) : (
        <button type="button" className={styles.textButton} onClick={editPorts}>{t("common.edit")}</button>
      )}>
        <div className={styles.portSettings}>
          <div className={styles.portFields}>
            {editingPorts ? <><FormField
              label={t("settings.proxy_port")}
              hint={t("settings.the_local_proxy_port_used_by_cur")}
            >
              <TextInput type="number" min={0} max={65535} step={1} value={proxyPort} onChange={(event) => setProxyPort(event.target.value)} />
            </FormField>
            <FormField label={t("settings.service_port")} hint={t("settings.the_local_management_service_por")}>
              <TextInput type="number" min={0} max={65535} step={1} value={servicePort} onChange={(event) => setServicePort(event.target.value)} />
            </FormField></> : <>
              <div className={styles.portValue}><strong>{t("settings.proxy_port")}</strong><span>{ports.proxy_port}</span></div>
              <div className={styles.portValue}><strong>{t("settings.service_port")}</strong><span>{ports.service_port}</span></div>
            </>}
          </div>
          <div className={styles.portFooter}>
            <small>{t("settings.if_a_port_is_occupied_a_new_rand")}</small>
          </div>
        </div>
      </TitledCard>
      <ProxySettingsCard settings={outboundProxy} draft={proxyDraft} editing={editingProxy} saving={savingProxy} onDraftChange={setProxyDraft} onEdit={editProxy} onCancel={cancelProxyEdit} onSave={() => void saveProxy()} />
    </>;
  } else if (activeTab === "observability") {
    body = (
      <>
        <TitledCard title={t("settings.call_observability")}>
          <div className={styles.settingRow}>
            <div>
              <strong>{t("settings.detailed_mode")}</strong>
              <small>{t("settings.also_store_complete_requests_and")}</small>
            </div>
            <Checkbox
              label=""
              checked={detailed}
              onChange={async (checked) => {
                await appStore.updateDetailed(checked);
                message(
                  checked
                    ? t("settings.detailed_mode_enabled")
                    : t("settings.detailed_mode_disabled")
                );
              }}
            />
          </div>
        </TitledCard>

        <TitledCard title={t("settings.quota_alert_threshold_title")}>
          <div className={styles.settingRow}>
            <div>
              <strong>{t("settings.quota_alert_threshold_label")}</strong>
              <small>{t("settings.quota_alert_threshold_hint")}</small>
            </div>
            <div className={styles.languageControl}>
              <Select
                value={quotaAlertThreshold}
                ariaLabel={t("settings.quota_alert_threshold_label")}
                options={[
                  { value: "0", label: t("settings.quota_threshold_off") },
                  { value: "5", label: "%5" },
                  { value: "10", label: "%10" },
                  { value: "15", label: t("settings.quota_threshold_default", { value: 15 }) },
                  { value: "20", label: "%20" },
                  { value: "25", label: "%25" },
                ]}
                onChange={(val) => {
                  setQuotaAlertThreshold(val);
                  localStorage.setItem("nexusor_quota_alert_threshold", val);
                  message(
                    t("settings.quota_threshold_saved", {
                      value: val === "0" ? t("settings.quota_threshold_off") : `%${val}`,
                    })
                  );
                }}
              />
            </div>
          </div>
        </TitledCard>

        <TitledCard title={t("settings.storage_management")}>
          <div className={styles.storageRow}>
            <div>
              <strong>{t("settings.statistics")}</strong>
              <small>
                {storage
                  ? `${t("settings.call_records_trace_records", {
                      calls: storage.call_count,
                      traces: storage.trace_count,
                    })}${
                      storage.database_bytes
                        ? ` · ${(storage.database_bytes / 1024 / 1024).toFixed(1)} MB`
                        : ""
                    }`
                  : t("settings.calculating")}
              </small>
            </div>
            <button
              type="button"
              className={styles.textButton}
              onClick={() => {
                setClearScope("details");
                setConfirmClear(true);
              }}
            >
              {t("settings.clear_storage")}
            </button>
          </div>
        </TitledCard>
      </>
    );
  } else if (activeTab === "import") {
    body = <BackupSettingsCard />;
  } else {
    body = <>
      <AppLifecycleSettingsCard />
      <TitledCard title={t("settings.language")}>
        <div className={styles.settingRow}>
          <div>
            <strong>{t("settings.display_language")}</strong>
            <small>{t("settings.uses_the_operating_system_langua", { language: locale === "tr-TR" ? "Türkçe" : locale === "zh-CN" ? "简体中文" : locale === "pt-BR" ? "Português (Brasil)" : "English" })}</small>
          </div>
          <div className={styles.languageControl}>
            <Select
              value={preference}
              ariaLabel={t("settings.display_language")}
              options={[
                { value: "system", label: t("settings.use_system_language") },
                { value: "tr-TR", label: "Türkçe" },
                { value: "en-US", label: "English" },
                { value: "zh-CN", label: "简体中文" },
                { value: "pt-BR", label: "Português (Brasil)" },
              ]}
              onChange={(value) => setLocalePreference(value as LocalePreference)}
            />
          </div>
        </div>
      </TitledCard>

      <TitledCard title={t("settings.storage_management")}>
        <div className={styles.storageRow}>
          <div>
            <strong>{t("settings.statistics")}</strong>
            <small>
              {storage
                ? `${t("settings.call_records_trace_records", {
                    calls: storage.call_count,
                    traces: storage.trace_count,
                  })}${
                    storage.database_bytes
                      ? ` · ${(storage.database_bytes / 1024 / 1024).toFixed(1)} MB`
                      : ""
                  }`
                : t("settings.calculating")}
            </small>
          </div>
          <button type="button" className={styles.textButton} onClick={() => { setClearScope("details"); setConfirmClear(true); }}>
            {t("settings.clear_storage")}
          </button>
        </div>
      </TitledCard>
    </>;
  }

  const content = (
    <div className={styles.page}>
      <div className={styles.tabs} role="tablist" aria-label={t("models.settings")}>
        {tabs.map((tab) => (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={activeTab === tab.id}
            className={activeTab === tab.id ? styles.tabActive : styles.tab}
            onClick={() => setTab(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </div>
      <div className={styles.tabPanel} role="tabpanel">
        {body}
      </div>
      <ConfirmDialog
        open={confirmClear}
        title={clearTitle}
        busy={clearing}
        cancelLabel={t("common.cancel")}
        confirmLabel={t("settings.confirm_clear")}
        onCancel={() => setConfirmClear(false)}
        onConfirm={() => void clearStorage()}
      >
        <div className={styles.confirmContent}>
          <Select
            value={clearScope}
            ariaLabel={t("settings.clear_scope")}
            options={[
              { value: "details", label: t("settings.only_clear_detailed_records") },
              { value: "all", label: t("settings.clear_all_statistics_2") },
            ]}
            onChange={(value) => setClearScope(value as StatisticsStorageScope)}
          />
          <small>{clearDescription}</small>
        </div>
      </ConfirmDialog>
    </div>
  );
  return <PageContent title={t("models.settings")} sections={[{ key: "settings", estimatedHeight: 900, content }]} />;
}
