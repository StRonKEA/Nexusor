import { useState } from "react";
import { useMessage } from "../../shared/ui/message";
import { useI18n } from "../../i18n/store";
import { useAppStore } from "../../shared/store/appStore";
import { providerLogos } from "../../shared/utils/providerIcons";
import styles from "./HomeAccessPanel.module.scss";

type ProtocolType = "openai" | "claude";

export function HomeAccessPanel() {
  const { ports } = useAppStore();
  const message = useMessage();
  useI18n();

  const [activeProtocol, setActiveProtocol] = useState<ProtocolType>("openai");
  const [copiedField, setCopiedField] = useState<string | null>(null);

  const servicePort = ports.service_port || 6817;
  const baseUrlOpenAI = `http://127.0.0.1:${servicePort}/v1`;
  const baseUrlClaude = `http://127.0.0.1:${servicePort}/v1/messages`;

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
    <div className={styles.accessPanel}>
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <div className={styles.statusDot} />
          <div>
            <h4 className={styles.title}>{t("home.api_gateway_title")}</h4>
            <p className={styles.subtitle}>{t("home.api_gateway_subtitle")}</p>
          </div>
        </div>
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

      <div className={styles.content}>
        <div className={styles.fieldRow}>
          <div className={styles.fieldInfo}>
            <span className={styles.fieldLabel}>
              {activeProtocol === "openai" ? "Base URL (OpenAI Compatible)" : "Endpoint URL (Anthropic / Claude)"}
            </span>
            <span className={styles.fieldHint}>
              {activeProtocol === "openai"
                ? "Cline, Continue, Aider, Chatbox vb. için Base URL"
                : "Claude Code CLI veya Anthropic SDK kullanan araçlar için"}
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

        {/* Quick Config Presets for Clients */}
        <div className={styles.presetsRow}>
          <span className={styles.presetsLabel}>{t("home.quick_configs")}:</span>
          <div className={styles.presetButtons}>
            <button
              type="button"
              className={styles.presetBtn}
              onClick={() =>
                copyToClipboard(
                  JSON.stringify(
                    {
                      apiProvider: "openai",
                      openAiBaseUrl: baseUrlOpenAI,
                      openAiApiKey: "nexusor-local",
                      openAiModelId: "auto-smart",
                    },
                    null,
                    2
                  ),
                  "cline"
                )
              }
            >
              <span>Cline / Roo Code JSON</span>
            </button>
            <button
              type="button"
              className={styles.presetBtn}
              onClick={() =>
                copyToClipboard(
                  `export OPENAI_BASE_URL="${baseUrlOpenAI}"\nexport OPENAI_API_KEY="nexusor-local"`,
                  "aider"
                )
              }
            >
              <span>Aider Env</span>
            </button>
            <button
              type="button"
              className={styles.presetBtn}
              onClick={() =>
                copyToClipboard(
                  `export ANTHROPIC_BASE_URL="http://127.0.0.1:${servicePort}"\nexport ANTHROPIC_API_KEY="nexusor-local"`,
                  "claudecode"
                )
              }
            >
              <span>Claude Code CLI Env</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
