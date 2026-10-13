import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api, type TabSettings } from "../../shared/api";
import { readDesktopSettings, writeHideCursorBuiltinModels } from "../../shared/native/appLifecycle";
import { CursorCaGate, CursorCaProvider } from "../models/CursorGates";
import { PageContent } from "../../shell/layout/PageContent";
import { ConfirmDialog } from "../../shared/ui/ConfirmDialog";
import { Card } from "../../shared/ui/Card";
import { Switch } from "../../shared/ui/Switch";
import { Button } from "../../shared/ui/Button";
import { useMessage } from "../../shared/ui/message";
import { appStore, useAppStore } from "../../shared/store/appStore";
import { TabSettingsCard } from "../settings/TabSettingsCard";
import { CommitSettingsCard } from "../settings/CommitSettingsCard";
import { CmdKSettingsCard } from "../settings/CmdKSettingsCard";
import { ExternalClientDetail } from "./ExternalClientDetail";
import { providerLogos } from "../../shared/utils/providerIcons";
import styles from "../models/CursorSettings.module.scss";
import pageStyles from "./CursorIntegrationPage.module.scss";
import masterStyles from "./ClientsMasterDetail.module.scss";
import { errorText } from "../../shared/utils/errorText";

type ClientID = "cursor" | "cline" | "continue" | "aider" | "claudecode" | "generic";

export function CursorIntegrationPage() {
  const { cursorHarness, cursorBusy } = useAppStore();
  const navigate = useNavigate();
  const message = useMessage();
  const [selectedClient, setSelectedClient] = useState<ClientID>("cursor");
  const [caCommand, setCaCommand] = useState<string | null>(null);
  const [waitingForCaRefresh, setWaitingForCaRefresh] = useState(false);
  const [confirmDisableTakeover, setConfirmDisableTakeover] = useState(false);
  const [repairing, setRepairing] = useState(false);

  // Core Cursor Feature Gates & Flags
  const [hideBuiltin, setHideBuiltin] = useState(true);
  const [loadingFlags, setLoadingFlags] = useState(true);

  // Tab Settings
  const [tabSettings, setTabSettings] = useState<TabSettings | null>(null);
  const [tabDraft, setTabDraft] = useState<TabSettings>({ mode: "public", address: "" });
  const [editingTab, setEditingTab] = useState(false);
  const [savingTab, setSavingTab] = useState(false);

  const caReady = cursorHarness?.ca === "ready";
  const cursorTakenOver = cursorHarness?.settings_applied ?? false;

  useEffect(() => {
    let disposed = false;
    void (async () => {
      try {
        const settings = await readDesktopSettings();
        if (disposed) return;
        setHideBuiltin(settings.hide_cursor_builtin_models ?? true);
      } catch {
        // default
      } finally {
        if (!disposed) setLoadingFlags(false);
      }
    })();

    void api.tabSettings().then((next) => {
      if (disposed) return;
      setTabSettings(next);
      setTabDraft(next);
    }).catch(() => {});

    return () => {
      disposed = true;
    };
  }, []);

  useEffect(() => {
    if (caCommand) void api.copyCursorText(caCommand);
  }, [caCommand]);

  const initializeCa = async () => {
    const status = await appStore.initializeCursorCa();
    if (status?.ca === "untrusted" && status.ca_install_command) {
      setCaCommand(status.ca_install_command);
    }
  };

  const refreshCa = async () => {
    await appStore.refresh();
    if (appStore.getSnapshot().cursorHarness?.ca !== "ready") {
      setWaitingForCaRefresh(false);
    }
  };

  const handleRepair = async () => {
    setRepairing(true);
    try {
      await api.repairCursorIntegration();
      await appStore.refresh();
      message(t("cursor.repair_success"));
    } catch (cause) {
      message(errorText(cause));
    } finally {
      setRepairing(false);
    }
  };

  const toggleHideBuiltin = async (enabled: boolean) => {
    try {
      await writeHideCursorBuiltinModels(enabled);
      setHideBuiltin(enabled);
      message(t("settings.settings_saved"));
    } catch (cause) {
      message(errorText(cause));
    }
  };

  const editTab = () => {
    if (!tabSettings) return;
    setTabDraft(tabSettings);
    setEditingTab(true);
  };

  const cancelTabEdit = () => {
    if (tabSettings) setTabDraft(tabSettings);
    setEditingTab(false);
  };

  const saveTab = async () => {
    setSavingTab(true);
    try {
      const saved = await api.setTabSettings({ ...tabDraft, address: tabDraft.address.trim() });
      setTabSettings(saved);
      setTabDraft(saved);
      setEditingTab(false);
      message(t("settings.tab_settings_saved"));
    } catch (cause) {
      message(errorText(cause));
    } finally {
      setSavingTab(false);
    }
  };

  const integrationLabel =
    cursorHarness?.integration === "enabled"
      ? t("cursor.enabled")
      : cursorHarness?.integration === "degraded"
      ? t("cursor.degraded")
      : t("cursor.disabled");

  const cursorContent = (
    <CursorCaProvider>
      <CursorCaGate
        busy={cursorBusy}
        waitingForRefresh={waitingForCaRefresh}
        onInitialize={() => void initializeCa()}
        onRefresh={() => void refreshCa()}
      >
        <div className={pageStyles.page}>
          <Card className={pageStyles.statusCard}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 14 }}>
              <strong style={{ fontSize: "14px", color: "#f7f8f8" }}>Cursor IDE Bağlantı Durumu</strong>
              <Button
                size="small"
                variant="secondary"
                disabled={cursorBusy || repairing}
                onClick={() => void handleRepair()}
              >
                {repairing ? t("common.processing") : t("cursor.repair_button")}
              </Button>
            </div>
            <div className={pageStyles.statusGrid}>
              <StatusItem
                label={t("cursor.ca_status")}
                value={caReady ? t("cursor.ready") : t("cursor.not_ready")}
                ok={caReady}
              />
              <StatusItem
                label={t("cursor.takeover_status")}
                value={cursorTakenOver ? t("cursor.taken_over") : t("cursor.not_taken_over")}
                ok={cursorTakenOver}
              />
              <StatusItem
                label={t("cursor.integration_status")}
                value={integrationLabel}
                ok={cursorHarness?.integration === "enabled"}
              />
              <StatusItem
                label={t("cursor.proxy_address")}
                value={cursorHarness?.proxy_url || t("cursor.not_generated")}
                ok={Boolean(cursorHarness?.proxy_url)}
              />
              <StatusItem
                label={t("cursor.local_models_in_use")}
                value={String(cursorHarness?.enabled_models ?? 0)}
                ok={(cursorHarness?.enabled_models ?? 0) > 0}
              />
            </div>
          </Card>

          {cursorTakenOver && (cursorHarness?.enabled_models ?? 0) === 0 && (
            <div className={pageStyles.warningCard}>
              <strong>{t("cursor.no_enabled_models_title")}</strong>
              <span>{t("cursor.no_enabled_models_body")}</span>
              <div style={{ display: "flex", gap: "8px", flexWrap: "wrap", marginTop: "8px" }}>
                <Button variant="primary" onClick={() => navigate("/models")}>
                  {t("home.go_to_models")}
                </Button>
                <Button variant="secondary" onClick={() => navigate("/providers")}>
                  {t("cursor.manage_providers")}
                </Button>
              </div>
            </div>
          )}

          {!cursorTakenOver && caReady && (
            <div className={styles.gate}>
              <strong>{t("cursor.cursor_is_not_taken_over_yet")}</strong>
              <span>{t("cursor.after_takeover_is_enabled_cursor")}</span>
              <Button
                variant="primary"
                disabled={cursorBusy}
                onClick={() => void appStore.setCursorEnabled(true)}
              >
                {t("cursor.enable_cursor_takeover")}
              </Button>
            </div>
          )}

          <div className={pageStyles.sectionBlock}>
            <div className={pageStyles.sectionHeaderRow}>
              <div>
                <h3 className={pageStyles.sectionTitle}>{t("cursor.model_behavior_title")}</h3>
              </div>
            </div>
            <Card className={pageStyles.configCard}>
              <div className={pageStyles.settingRow}>
                <div>
                  <strong>{t("cursor.hide_builtin_models")}</strong>
                  <small>{t("cursor.hide_builtin_models_desc")}</small>
                </div>
                <Switch
                  checked={hideBuiltin}
                  disabled={loadingFlags}
                  label={t("cursor.hide_builtin_models")}
                  onChange={(enabled) => void toggleHideBuiltin(enabled)}
                />
              </div>
            </Card>
          </div>

          <TabSettingsCard
            settings={tabSettings}
            draft={tabDraft}
            editing={editingTab}
            saving={savingTab}
            onDraftChange={setTabDraft}
            onEdit={editTab}
            onCancel={cancelTabEdit}
            onSave={() => void saveTab()}
          />

          <CommitSettingsCard />
          <CmdKSettingsCard />
        </div>
      </CursorCaGate>
    </CursorCaProvider>
  );

  return (
    <>
      <PageContent
        title={t("nav.clients_and_ides")}
        sections={[
          {
            key: "clients-master-detail",
            estimatedHeight: 800,
            content: (
              <div className={masterStyles.unifiedLayout}>
                {/* ── Sol Sütun: Dikey İstemci Listesi (Vertical Sidebar) ── */}
                <div className={masterStyles.sidebar}>
                  <button
                    type="button"
                    className={`${masterStyles.clientItem} ${selectedClient === "cursor" ? masterStyles.clientItemActive : ""}`}
                    onClick={() => setSelectedClient("cursor")}
                  >
                    <img src={providerLogos.antigravity} alt="" className={masterStyles.clientLogo} />
                    <div className={masterStyles.clientInfo}>
                      <span className={masterStyles.clientTitle}>Cursor IDE</span>
                      <span className={masterStyles.clientSubtitle}>{cursorTakenOver ? "Vekil Sunucu Aktif" : "Bağlantı Bekliyor"}</span>
                    </div>
                    {cursorTakenOver && <span className={masterStyles.activeIndicator} title="Aktif" />}
                  </button>

                  <button
                    type="button"
                    className={`${masterStyles.clientItem} ${selectedClient === "cline" ? masterStyles.clientItemActive : ""}`}
                    onClick={() => setSelectedClient("cline")}
                  >
                    <img src={providerLogos.openai} alt="" className={masterStyles.clientLogo} />
                    <div className={masterStyles.clientInfo}>
                      <span className={masterStyles.clientTitle}>Cline / Roo Code</span>
                      <span className={masterStyles.clientSubtitle}>VS Code Uzantısı</span>
                    </div>
                  </button>

                  <button
                    type="button"
                    className={`${masterStyles.clientItem} ${selectedClient === "continue" ? masterStyles.clientItemActive : ""}`}
                    onClick={() => setSelectedClient("continue")}
                  >
                    <img src={providerLogos.openai} alt="" className={masterStyles.clientLogo} />
                    <div className={masterStyles.clientInfo}>
                      <span className={masterStyles.clientTitle}>Continue.dev</span>
                      <span className={masterStyles.clientSubtitle}>VS Code / JetBrains</span>
                    </div>
                  </button>

                  <button
                    type="button"
                    className={`${masterStyles.clientItem} ${selectedClient === "aider" ? masterStyles.clientItemActive : ""}`}
                    onClick={() => setSelectedClient("aider")}
                  >
                    <img src={providerLogos.openai} alt="" className={masterStyles.clientLogo} />
                    <div className={masterStyles.clientInfo}>
                      <span className={masterStyles.clientTitle}>Aider</span>
                      <span className={masterStyles.clientSubtitle}>Terminal Ajanı</span>
                    </div>
                  </button>

                  <button
                    type="button"
                    className={`${masterStyles.clientItem} ${selectedClient === "claudecode" ? masterStyles.clientItemActive : ""}`}
                    onClick={() => setSelectedClient("claudecode")}
                  >
                    <img src={providerLogos.anthropic} alt="" className={masterStyles.clientLogo} />
                    <div className={masterStyles.clientInfo}>
                      <span className={masterStyles.clientTitle}>Claude Code</span>
                      <span className={masterStyles.clientSubtitle}>Resmi Anthropic CLI</span>
                    </div>
                  </button>

                  <button
                    type="button"
                    className={`${masterStyles.clientItem} ${selectedClient === "generic" ? masterStyles.clientItemActive : ""}`}
                    onClick={() => setSelectedClient("generic")}
                  >
                    <img src={providerLogos.openrouter} alt="" className={masterStyles.clientLogo} />
                    <div className={masterStyles.clientInfo}>
                      <span className={masterStyles.clientTitle}>Diğer İstemciler</span>
                      <span className={masterStyles.clientSubtitle}>Chatbox / WebUI / Python</span>
                    </div>
                  </button>
                </div>

                {/* ── Sağ Sütun: Seçilen İstemcinin Yapılandırma Paneli ── */}
                <div className={masterStyles.detailPane}>
                  {selectedClient === "cursor" ? cursorContent : <ExternalClientDetail selected={selectedClient} />}
                </div>
              </div>
            ),
          },
        ]}
      />

      <ConfirmDialog
        open={confirmDisableTakeover}
        title={t("cursor.disable_cursor_takeover_2")}
        cancelLabel={t("common.cancel")}
        confirmLabel={t("cursor.disable_takeover")}
        onCancel={() => setConfirmDisableTakeover(false)}
        onConfirm={() => {
          setConfirmDisableTakeover(false);
          void appStore.setCursorEnabled(false);
        }}
      >
        <p>{t("cursor.disabling_this_will_remove_curso")}</p>
      </ConfirmDialog>

      <ConfirmDialog
        open={caCommand !== null}
        title={t("cursor.install_local_ca")}
        cancelLabel={t("common.close")}
        confirmLabel={t("cursor.open_terminal")}
        onCancel={() => setCaCommand(null)}
        onConfirm={handleRepair}
      >
        <p>{t("cursor.the_install_command_has_been_cop")}</p>
      </ConfirmDialog>
    </>
  );
}

function StatusItem({ label, value, ok }: { label: string; value: string; ok?: boolean }) {
  return (
    <div className={pageStyles.statusItem}>
      <small>{label}</small>
      <strong data-ok={ok ? "true" : undefined}>{value}</strong>
    </div>
  );
}
