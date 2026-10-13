import { useState } from "react";
import { useMessage } from "../../shared/ui/message";
import { useI18n } from "../../i18n/store";
import { appStore, useAppStore } from "../../shared/store/appStore";
import { Switch } from "../../shared/ui/Switch";
import { ConfirmDialog } from "../../shared/ui/ConfirmDialog";
import { providerLogos } from "../../shared/utils/providerIcons";
import styles from "./HomeAccessPanel.module.scss";

type ProtocolType = "openai" | "claude";

export function HomeAccessPanel() {
  const { ports, cursorHarness, cursorBusy } = useAppStore();
  const message = useMessage();
  useI18n();

  const [activeProtocol, setActiveProtocol] = useState<ProtocolType>("openai");
  const [copiedField, setCopiedField] = useState<string | null>(null);
  const [confirmDisableTakeover, setConfirmDisableTakeover] = useState(false);

  const servicePort = ports.service_port || 6817;
  const baseUrlOpenAI = `http://127.0.0.1:${servicePort}/v1`;
  const baseUrlClaude = `http://127.0.0.1:${servicePort}`;

  const cursorTakenOver = cursorHarness?.settings_applied ?? false;
  const caReady = cursorHarness?.ca === "ready";

  const copyToClipboard = async (text: string, fieldName: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedField(fieldName);
      message(t("common.copied_to_clipboard"));
      setTimeout(() => setCopiedField(null), 2000);
    } catch {
      message(t("common.copy_failed"));
    }
  };

  return (
    <>
      <div className={styles.accessPanel}>
        <div className={styles.header}>
          <div className={styles.headerLeft}>
            <div className={styles.statusDot} data-active={cursorTakenOver ? "true" : "false"} />
            <h4 className={styles.title}>{t("home.api_gateway_title")}</h4>
          </div>

          <div className={styles.headerRight}>
            {/* ── Cursor BYOK / Proxy Açma-Kapama Anahtarı (Master Switch) ── */}
            <div className={styles.takeoverControl}>
              <span className={styles.takeoverStatusText} data-active={cursorTakenOver ? "true" : "false"}>
                {cursorTakenOver ? "Cursor BYOK Aktif" : "Cursor BYOK Kapalı"}
              </span>
              <Switch
                checked={cursorTakenOver}
                disabled={cursorBusy || (!cursorTakenOver && !caReady)}
                label="Cursor BYOK"
                onChange={(enabled) => {
                  if (enabled) void appStore.setCursorEnabled(true);
                  else setConfirmDisableTakeover(true);
                }}
              />
            </div>

            {/* ── Protokol Sekmeleri ── */}
            <div className={styles.protocolTabs}>
              <button
                type="button"
                className={`${styles.tabBtn} ${activeProtocol === "openai" ? styles.tabBtnActive : ""}`}
                onClick={() => setActiveProtocol("openai")}
              >
                <img src={providerLogos.openai} alt="" className={styles.protocolIcon} />
                <span>OpenAI API</span>
              </button>
              <button
                type="button"
                className={`${styles.tabBtn} ${activeProtocol === "claude" ? styles.tabBtnActive : ""}`}
                onClick={() => setActiveProtocol("claude")}
              >
                <img src={providerLogos.anthropic} alt="" className={styles.protocolIcon} />
                <span>Claude API</span>
              </button>
            </div>
          </div>
        </div>

        <div className={styles.content}>
          <div className={styles.fieldRow}>
            <div className={styles.fieldInfo}>
              <span className={styles.fieldLabel}>
                {activeProtocol === "openai" ? "Base URL (OpenAI Protocol)" : "Base URL (Anthropic Protocol)"}
              </span>
              <span className={styles.fieldHint}>
                {activeProtocol === "openai"
                  ? "Cline, Continue, Aider, Chatbox vb. için Base URL"
                  : "Claude Code CLI, Anthropic SDK veya Claude destekli araçlar için Base URL"}
              </span>
            </div>
            <div className={styles.copyBox}>
              <code className={styles.codeText}>
                {activeProtocol === "openai" ? baseUrlOpenAI : baseUrlClaude}
              </code>
              <button
                type="button"
                className={styles.copyBtn}
                onClick={() =>
                  copyToClipboard(
                    activeProtocol === "openai" ? baseUrlOpenAI : baseUrlClaude,
                    "url"
                  )
                }
              >
                {copiedField === "url" ? t("common.copied") : t("common.copy")}
              </button>
            </div>
          </div>
        </div>
      </div>

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
    </>
  );
}
