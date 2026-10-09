import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import type { LlmCall } from "../../shared/api";
import { CallTable } from "./CallTable";
import { PageContent } from "../../shell/layout/PageContent";
import { appStore, useAppStore } from "../../shared/store/appStore";
import { Select } from "../../shared/ui/Select";
import styles from "./CallsPage.module.scss";

const CALL_REFRESH_INTERVAL_MS = 2_000;

function getProviderDisplayName(call: LlmCall): string {
  const hash = call.model_hash || "";
  if (hash.includes("antigravity")) return "Google Antigravity";
  if (hash.includes("copilot")) return "GitHub Copilot";
  if (hash.includes("codex")) return "OpenAI Codex";
  if (hash.includes("grok")) return "xAI Grok";
  if (hash.includes("kimi")) return "Moonshot Kimi";
  if (hash.includes("claude_code")) return "Claude Code";
  if (call.provider_type === "openai-chat") return "OpenAI";
  if (call.provider_type === "anthropic") return "Anthropic";
  if (call.provider_type === "plugin") return "Yerleşik Eklenti";
  return call.provider_type || "Bilinmeyen";
}

export function CallsPage() {
  const { calls } = useAppStore();
  const navigate = useNavigate();
  const [statusFilter, setStatusFilter] = useState("all");
  const [modelFilter, setModelFilter] = useState("all");
  const [providerFilter, setProviderFilter] = useState("all");
  const [searchQuery, setSearchQuery] = useState("");

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
      const matchError = call.error_message?.toLowerCase().includes(q) || call.error_kind?.toLowerCase().includes(q);
      if (!matchId && !matchModel && !matchError) return false;
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
        <input
          type="text"
          placeholder="İstek ID veya hata ara..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          style={{
            height: "30px",
            padding: "0 10px",
            fontSize: "12px",
            borderRadius: "6px",
            border: "1px solid rgba(255, 255, 255, 0.1)",
            background: "rgba(255, 255, 255, 0.04)",
            color: "var(--vscode-foreground)",
            outline: "none",
            minWidth: "180px",
          }}
        />
      </div>
      <div className={styles.filterMeta}>
        {filtered.length} {t("calls.calls")}
      </div>
    </div>
    <div className={styles.tableWrap}>
      <CallTable
        calls={filtered}
        onDetails={(call: LlmCall) => navigate(`/calls/${encodeURIComponent(call.call_id)}`)}
      />
    </div>
  </div>;

  return <PageContent fixed title={t("calls.calls")} contentClassName={styles.pageContent} sections={[{ key: "calls", estimatedHeight: 720, content }]} />;
}
