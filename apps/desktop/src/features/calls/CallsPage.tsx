import { useEffect, useMemo, useState } from "react";
import { api, type CallDetail, type LlmCall } from "../../shared/api";
import { CallTable } from "./CallTable";
import { CallDetails } from "./CallDetails";
import { PageContent } from "../../shell/layout/PageContent";
import { appStore, useAppStore } from "../../shared/store/appStore";
import { Select } from "../../shared/ui/Select";
import { TextInput } from "../../shared/ui/FormControls";
import { Button } from "../../shared/ui/Button";
import { Icon } from "../../shared/ui/Icon";
import { closeIcon } from "../../shared/ui/icons";
import styles from "./CallsPage.module.scss";

const CALL_REFRESH_INTERVAL_MS = 2_000;

function getProviderDisplayName(call: LlmCall): string {
  const hash = (call.model_hash || "").toLowerCase();
  if (hash.includes("antigravity")) return "Google Antigravity";
  if (hash.includes("copilot")) return "GitHub Copilot";
  if (hash.includes("codex")) return "OpenAI Codex";
  if (hash.includes("grok")) return "xAI Grok";
  if (hash.includes("kimi")) return "Moonshot Kimi";
  if (hash.includes("claude_code") || hash.includes("claude-code")) return "Claude Code";
  if (hash.includes("deepseek")) return "DeepSeek";
  if (hash.includes("groq")) return "Groq";
  if (hash.includes("nvidia")) return "NVIDIA NIM";
  if (hash.includes("opencode")) return "OpenCode";
  if (hash.includes("ollama")) return "Ollama";
  if (hash.includes("openrouter")) return "OpenRouter";
  if (call.provider_type === "openai-chat") return "OpenAI (Custom)";
  if (call.provider_type === "anthropic") return "Anthropic (Custom)";
  if (call.provider_type === "plugin") return t("calls.builtin_plugin");
  return call.provider_type || t("calls.unknown");
}

export function CallsPage() {
  const { calls } = useAppStore();
  const [statusFilter, setStatusFilter] = useState("all");
  const [modelFilter, setModelFilter] = useState("all");
  const [providerFilter, setProviderFilter] = useState("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedCallId, setSelectedCallId] = useState<string | null>(null);
  const [selectedDetail, setSelectedDetail] = useState<CallDetail | null>(null);
  const [loadingDetail, setLoadingDetail] = useState(false);

  useEffect(() => {
    let cancelled = false;
    if (!selectedCallId) {
      setSelectedDetail(null);
      return;
    }
    setLoadingDetail(true);
    void api.call(selectedCallId).then((data) => {
      if (!cancelled) {
        setSelectedDetail(data);
        setLoadingDetail(false);
      }
    }).catch(() => {
      if (!cancelled) setLoadingDetail(false);
    });
    return () => {
      cancelled = true;
    };
  }, [selectedCallId]);

  useEffect(() => {
    if (!selectedCallId) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setSelectedCallId(null);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [selectedCallId]);

  useEffect(() => {
    let disposed = false;
    const refreshCalls = () => {
      if (!disposed && document.visibilityState === "visible") {
        void appStore.refreshCalls();
      }
    };

    refreshCalls();
    const interval = window.setInterval(refreshCalls, CALL_REFRESH_INTERVAL_MS);
    window.addEventListener("focus", refreshCalls);
    document.addEventListener("visibilitychange", refreshCalls);
    return () => {
      disposed = true;
      window.clearInterval(interval);
      window.removeEventListener("focus", refreshCalls);
      document.removeEventListener("visibilitychange", refreshCalls);
    };
  }, []);

  const modelOptions = useMemo(() => {
    const values = [...new Set(calls.map((call) => call.display_name || call.model_id).filter(Boolean))];
    return [{ value: "all", label: t("calls.all_models") }, ...values.map((value) => ({ value, label: value }))];
  }, [calls]);
  const providerOptions = useMemo(() => {
    const values = [...new Set(calls.map(getProviderDisplayName).filter(Boolean))];
    return [{ value: "all", label: t("calls.all_providers") }, ...values.map((value) => ({ value, label: value }))];
  }, [calls]);
  const filtered = useMemo(() => calls.filter((call) => {
    if (statusFilter !== "all" && call.status !== statusFilter) return false;
    if (modelFilter !== "all" && (call.display_name || call.model_id) !== modelFilter) return false;
    if (providerFilter !== "all" && getProviderDisplayName(call) !== providerFilter) return false;
    if (searchQuery.trim()) {
      const q = searchQuery.toLowerCase();
      const matchId = call.call_id?.toLowerCase().includes(q);
      const matchModel = (call.display_name || call.model_id)?.toLowerCase().includes(q);
      const matchProvider = getProviderDisplayName(call).toLowerCase().includes(q);
      const matchError = call.error_message?.toLowerCase().includes(q) || call.error_kind?.toLowerCase().includes(q);
      if (!matchId && !matchModel && !matchProvider && !matchError) return false;
    }
    return true;
  }), [calls, modelFilter, providerFilter, statusFilter, searchQuery]);

  const content = <div className={styles.page}>
    <div className={styles.filterBar}>
      <div className={styles.filterGroup}>
        <Select
          ariaLabel={t("common.status")}
          value={statusFilter}
          options={[
            { value: "all", label: t("calls.all_statuses") },
            { value: "completed", label: t("calls.success") },
            { value: "failed", label: t("calls.failed") },
            { value: "running", label: t("calls.in_progress") },
            { value: "cancelled", label: t("calls.cancelled") },
          ]}
          onChange={setStatusFilter}
        />
        <Select ariaLabel={t("calls.model")} value={modelFilter} options={modelOptions} onChange={setModelFilter} />
        <Select ariaLabel={t("calls.providers")} value={providerFilter} options={providerOptions} onChange={setProviderFilter} />
        <TextInput
          placeholder={t("calls.search_placeholder")}
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          className={styles.searchInput}
        />
      </div>
      <div className={styles.filterMeta}>
        {filtered.length} {t("calls.calls")}
      </div>
    </div>
    <div className={styles.tableWrap}>
      <div className={styles.masterDetailLayout}>
        <div className={styles.masterList}>
          <CallTable
            calls={filtered}
            onDetails={(call: LlmCall) => setSelectedCallId(call.call_id)}
          />
        </div>
        {selectedCallId && (
          <div className={styles.detailDrawer}>
            <div className={styles.detailDrawerHeader}>
              <strong>{selectedDetail?.call.display_name || selectedCallId}</strong>
              <Button size="small" variant="secondary" onClick={() => setSelectedCallId(null)}>
                <Icon icon={closeIcon} size="1em" />
              </Button>
            </div>
            <div className={styles.detailDrawerContent}>
              {loadingDetail ? (
                <div style={{ padding: 16, color: "var(--vscode-descriptionForeground)" }}>
                  {t("calls.loading_call_details")}
                </div>
              ) : selectedDetail ? (
                <CallDetails detail={selectedDetail} />
              ) : (
                <div style={{ padding: 16, color: "var(--vscode-descriptionForeground)" }}>
                  {t("calls.unable_to_load_call_details")}
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  </div>;

  return <PageContent fixed title={t("calls.calls")} contentClassName={styles.pageContent} sections={[{ key: "calls", estimatedHeight: 720, content }]} />;
}
