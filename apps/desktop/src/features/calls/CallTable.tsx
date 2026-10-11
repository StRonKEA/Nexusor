import type { LlmCall } from "../../shared/api";
import controls from "../../shared/ui/Controls.module.scss";
import { DataTable, type DataTableColumn } from "../../shared/ui/DataTable";
import { Icon } from "../../shared/ui/Icon";
import { TooltipTrigger } from "../../shared/ui/TooltipTrigger";
import { eyeIcon } from "../../shared/ui/icons";
import styles from "./CallTable.module.scss";

const formatDuration = (ms: number | null) => {
  if (ms == null) return "-";
  if (ms >= 1000) return `${(ms / 1000).toFixed(2)} s`;
  return `${ms} ms`;
};

const formatTime = (ms: number) => {
  const d = new Date(ms);
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
};

const formatTokens = (total: number | null, input: number | null, output: number | null) => {
  if (total == null && input == null && output == null) return "-";
  if (total != null) {
    if (total >= 1000) return `${(total / 1000).toFixed(1)}k`;
    return `${total}`;
  }
  return "-";
};

export function CallTable({
  calls,
  onDetails,
}: {
  calls: LlmCall[];
  onDetails: (call: LlmCall) => void;
}) {
  const columns: DataTableColumn<LlmCall>[] = [
    {
      key: "status",
      header: t("common.status"),
      render: (call) => {
        const errorHint = call.error_kind || call.error_message;
        const statusEl = (
          <span
            className={[styles.status, styles[call.status]]
              .filter(Boolean)
              .join(" ")}
          >
            {call.status}
            {call.http_status ? ` (${call.http_status})` : ""}
          </span>
        );
        return errorHint ? (
          <TooltipTrigger label={`Hata Nedeni: ${errorHint}`}>
            {statusEl}
          </TooltipTrigger>
        ) : (
          statusEl
        );
      },
    },
    {
      key: "display_name",
      header: t("calls.model_name"),
      render: (call) => (
        <div className={styles.modelCol}>
          <strong className={styles.modelName}>
            {call.display_name || call.model_id}
          </strong>
          {call.display_name && call.display_name !== call.model_id && (
            <small className={styles.modelSub}>
              {call.model_id}
            </small>
          )}
        </div>
      ),
      title: (call) => call.model_id,
    },
    {
      key: "created_at",
      header: t("calls.time"),
      render: (call) => (
        <TooltipTrigger label={new Date(call.created_at_ms).toLocaleString()}>
          <span className={styles.timeCol}>
            {formatTime(call.created_at_ms)}
          </span>
        </TooltipTrigger>
      ),
    },
    {
      key: "duration",
      header: t("calls.duration"),
      render: (call) => (
        <span className={styles.monoCol}>
          {formatDuration(call.duration_ms)}
        </span>
      ),
    },
    {
      key: "total_tokens",
      header: t("calls.total_tokens"),
      render: (call) => (
        <TooltipTrigger
          label={`${t("home.input_non_cached")}: ${call.input_tokens ?? "-"} · ${t("home.model_output")}: ${call.output_tokens ?? "-"}`}
        >
          <span className={styles.monoCol}>
            {formatTokens(call.total_tokens, call.input_tokens, call.output_tokens)}
          </span>
        </TooltipTrigger>
      ),
    },
    {
      key: "actions",
      header: t("calls.actions"),
      sticky: "right",
      render: (call) => (
        <TooltipTrigger label={t("calls.view_details")}>
          <button
            className={controls.iconButton}
            aria-label={t("calls.view_details")}
            onClick={() => onDetails(call)}
          >
            <Icon icon={eyeIcon} size="1.1em" />
          </button>
        </TooltipTrigger>
      ),
    },
  ];

  return <DataTable rows={calls} columns={columns} rowKey={(call) => call.call_id} />;
}
