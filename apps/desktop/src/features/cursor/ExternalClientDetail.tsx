import { useState } from "react";
import { useMessage } from "../../shared/ui/message";
import { useI18n } from "../../i18n/store";
import { useAppStore } from "../../shared/store/appStore";
import styles from "./ClientsMasterDetail.module.scss";

type ClientKey = "cline" | "continue" | "aider" | "claudecode" | "generic";

export function ExternalClientDetail({ selected }: { selected: ClientKey }) {
  const { ports } = useAppStore();
  const message = useMessage();
  useI18n();

  const [copiedField, setCopiedField] = useState<string | null>(null);

  const servicePort = ports.service_port || 6817;
  const baseUrlOpenAI = `http://127.0.0.1:${servicePort}/v1`;
  const baseUrlClaude = `http://127.0.0.1:${servicePort}`;

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
    <div className={styles.configCard}>
      {selected === "cline" && (
        <div className={styles.clientContent}>
          <div className={styles.cardHeader}>
            <h4>Cline / Roo Code (VS Code) Yapılandırması</h4>
            <p>VS Code üzerinde Cline uzantısını yerel Nexusor modellerine bağlamak için aşağıdaki ayarları kullanın.</p>
          </div>
          <div className={styles.fieldGrid}>
            <div className={styles.fieldItem}>
              <span className={styles.fieldTitle}>API Provider:</span>
              <code className={styles.fieldValue}>OpenAI Compatible</code>
            </div>
            <div className={styles.fieldItem}>
              <span className={styles.fieldTitle}>Base URL:</span>
              <div className={styles.copyRow}>
                <code className={styles.fieldValue}>{baseUrlOpenAI}</code>
                <button type="button" className={styles.copyBtn} onClick={() => copyToClipboard(baseUrlOpenAI, "cline-url")}>
                  {copiedField === "cline-url" ? "Kopyalandı" : "Kopyala"}
                </button>
              </div>
            </div>
            <div className={styles.fieldItem}>
              <span className={styles.fieldTitle}>API Key:</span>
              <code className={styles.fieldValue}>nexusor-local</code>
            </div>
            <div className={styles.fieldItem}>
              <span className={styles.fieldTitle}>Model ID:</span>
              <code className={styles.fieldValue}>auto-smart (veya gemini-3.7-flash, gpt-4o, claude-3-7-sonnet)</code>
            </div>
          </div>
          <div className={styles.actionRow}>
            <button
              type="button"
              className={styles.primaryCopyBtn}
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
                  "cline-json"
                )
              }
            >
              {copiedField === "cline-json" ? "Tüm JSON Ayarı Kopyalandı!" : "Hazır Cline JSON Ayarını Kopyala"}
            </button>
          </div>
        </div>
      )}

      {selected === "continue" && (
        <div className={styles.clientContent}>
          <div className={styles.cardHeader}>
            <h4>Continue.dev (VS Code / JetBrains) Yapılandırması</h4>
            <p><code>config.json</code> dosyanızdaki models dizisine aşağıdaki bloğu ekleyin.</p>
          </div>
          <div className={styles.codeSnippetBox}>
            <pre>
{`{
  "title": "Nexusor (Auto-Smart)",
  "provider": "openai",
  "model": "auto-smart",
  "apiBase": "${baseUrlOpenAI}",
  "apiKey": "nexusor-local"
}`}
            </pre>
          </div>
          <div className={styles.actionRow}>
            <button
              type="button"
              className={styles.primaryCopyBtn}
              onClick={() =>
                copyToClipboard(
                  `{\n  "title": "Nexusor (Auto-Smart)",\n  "provider": "openai",\n  "model": "auto-smart",\n  "apiBase": "${baseUrlOpenAI}",\n  "apiKey": "nexusor-local"\n}`,
                  "continue-json"
                )
              }
            >
              {copiedField === "continue-json" ? "Kopyalandı!" : "Continue Model Bloğunu Kopyala"}
            </button>
          </div>
        </div>
      )}

      {selected === "aider" && (
        <div className={styles.clientContent}>
          <div className={styles.cardHeader}>
            <h4>Aider Terminal Ajanı</h4>
            <p>Terminalinizde Aider'ı Nexusor üzerinden çalıştırmak için ortam değişkenlerini ayarlayın.</p>
          </div>
          <div className={styles.codeSnippetBox}>
            <pre>
{`export OPENAI_BASE_URL="${baseUrlOpenAI}"
export OPENAI_API_KEY="nexusor-local"
aider --model auto-smart`}
            </pre>
          </div>
          <div className={styles.actionRow}>
            <button
              type="button"
              className={styles.primaryCopyBtn}
              onClick={() =>
                copyToClipboard(
                  `export OPENAI_BASE_URL="${baseUrlOpenAI}"\nexport OPENAI_API_KEY="nexusor-local"\naider --model auto-smart`,
                  "aider-cmd"
                )
              }
            >
              {copiedField === "aider-cmd" ? "Kopyalandı!" : "Terminal Komutlarını Kopyala"}
            </button>
          </div>
        </div>
      )}

      {selected === "claudecode" && (
        <div className={styles.clientContent}>
          <div className={styles.cardHeader}>
            <h4>Anthropic Claude Code CLI</h4>
            <p>Resmi Claude Code CLI'ın tüm isteklerini yerel Nexusor motoruna yönlendirir.</p>
          </div>
          <div className={styles.codeSnippetBox}>
            <pre>
{`export ANTHROPIC_BASE_URL="${baseUrlClaude}"
export ANTHROPIC_API_KEY="nexusor-local"
claude`}
            </pre>
          </div>
          <div className={styles.actionRow}>
            <button
              type="button"
              className={styles.primaryCopyBtn}
              onClick={() =>
                copyToClipboard(
                  `export ANTHROPIC_BASE_URL="${baseUrlClaude}"\nexport ANTHROPIC_API_KEY="nexusor-local"\nclaude`,
                  "claude-cmd"
                )
              }
            >
              {copiedField === "claude-cmd" ? "Kopyalandı!" : "Claude CLI Komutlarını Kopyala"}
            </button>
          </div>
        </div>
      )}

      {selected === "generic" && (
        <div className={styles.clientContent}>
          <div className={styles.cardHeader}>
            <h4>Genel İstemciler (Chatbox, LibreChat, OpenWebUI, Python)</h4>
            <p>Standart OpenAI veya Anthropic uyumlu herhangi bir yazılım için bağlantı uç noktaları.</p>
          </div>
          <div className={styles.fieldGrid}>
            <div className={styles.fieldItem}>
              <span className={styles.fieldTitle}>OpenAI Uyumlu Base URL:</span>
              <div className={styles.copyRow}>
                <code className={styles.fieldValue}>{baseUrlOpenAI}</code>
                <button type="button" className={styles.copyBtn} onClick={() => copyToClipboard(baseUrlOpenAI, "gen-openai")}>
                  {copiedField === "gen-openai" ? "Kopyalandı" : "Kopyala"}
                </button>
              </div>
            </div>
            <div className={styles.fieldItem}>
              <span className={styles.fieldTitle}>Anthropic Uyumlu Base URL:</span>
              <div className={styles.copyRow}>
                <code className={styles.fieldValue}>{baseUrlClaude}</code>
                <button type="button" className={styles.copyBtn} onClick={() => copyToClipboard(baseUrlClaude, "gen-claude")}>
                  {copiedField === "gen-claude" ? "Kopyalandı" : "Kopyala"}
                </button>
              </div>
            </div>
            <div className={styles.fieldItem}>
              <span className={styles.fieldTitle}>API Key:</span>
              <code className={styles.fieldValue}>nexusor-local</code>
            </div>
            <div className={styles.fieldItem}>
              <span className={styles.fieldTitle}>Desteklenen Modeller:</span>
              <code className={styles.fieldValue}>auto-smart, auto, gpt-4o, claude-3-7-sonnet, gemini-3.7-flash</code>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
