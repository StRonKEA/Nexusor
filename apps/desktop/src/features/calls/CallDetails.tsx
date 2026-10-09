import type { CallDetail } from "../../shared/api";
import { JsonEditor } from "../../shared/ui/JsonEditor";
import { Tabs, type TabItem } from "../../shared/ui/Tabs";
import styles from "./CallDetails.module.scss";

const show = (value: string | number | null) => value ?? "-";
const timing = (value: number | null) => value == null ? "-" : `${value} ms`;

export function CallDetails({ detail }: { detail: CallDetail }) {
  const { call, request, response_chunks: chunks, cursor_trace: cursorTrace } = detail;
  const responseBody = chunks.map((chunk) => chunk.data).join("");
  const responseBytes = chunks.reduce((total, chunk) => total + chunk.byte_count, 0);
  const fields: Array<[string, string | number]> = [
    ["Call ID", call.call_id],
    [t("calls.call_type"), call.call_kind === "cursor_official" ? t("calls.cursor_official") : "LLM"],
    [t("calls.route"), call.route === "cursor_official" ? t("calls.cursor_official") : "Nexusor"],
    ["Run ID", call.run_id],
    ["Conversation ID", call.conversation_id],
    [t("calls.provider_call_sequence"), call.provider_call_index],
    ["Model Hash", show(call.model_hash)],
    [t("calls.provider_type"), call.provider_type],
    [t("calls.provider_url"), call.provider_url],
    [t("calls.final_request_type"), call.request_type],
    [t("calls.final_request_url"), call.request_url],
    ["Model ID", call.model_id],
    [t("calls.display_name"), call.display_name],
    [t("calls.reasoning_effort"), show(call.reasoning_effort)],
    ["Fast", call.fast == null ? "-" : call.fast ? t("calls.yes") : t("calls.no")],
    [t("common.status"), call.status],
    ["Finish Reason", show(call.finish_reason)],
    ["HTTP Status", show(call.http_status)],
    ["Created At", `${call.created_at_ms} · ${new Date(call.created_at_ms).toLocaleString()}`],
    [t("calls.duration"), timing(call.duration_ms)],
    ["TTFB", timing(call.ttfb_ms)],
    ["TTFR", timing(call.ttfr_ms)],
    ["TTFT", timing(call.ttft_ms)],
    ["Input Token", show(call.input_tokens)],
    ["Output Token", show(call.output_tokens)],
    ["Total Token", show(call.total_tokens)],
    ["Cache Read Token", show(call.cache_read_tokens)],
    ["Cache Write Token", show(call.cache_write_tokens)],
    ["Reasoning Token", show(call.reasoning_tokens)],
    [t("calls.messages"), call.message_count],
    [t("calls.tools"), call.tool_count],
    [t("calls.detailed_records"), call.detailed ? t("calls.yes") : t("calls.no")],
    ["Error Kind", show(call.error_kind)],
    ["Error Message", show(call.error_message)],
  ];

  const tabs: TabItem[] = [
    { value: "call", label: t("calls.call_information"), content: <section>
      <dl className={styles.details}>{fields.map(([label, value]) => <div key={label}><dt>{label}</dt><dd><code>{value}</code></dd></div>)}</dl>
    </section> },
    { value: "request", label: t("calls.request"), content: <section>
      {request ? <>
        <div className={styles.meta}>{t("calls.bytes")}: {request.byte_count}</div>
        <h4>{t("calls.request_headers")}</h4><JsonEditor ariaLabel={t("calls.request_headers")} value={JSON.stringify(request.headers)} readOnly />
        <h4>{t("calls.request_body")}</h4><JsonEditor ariaLabel={t("calls.request_body")} value={JSON.stringify(request.body)} readOnly detail />
      </> : <div className={styles.empty}>{t("calls.request_content_was_not_recorded")}</div>}
    </section> },
    { value: "response", label: t("calls.response_stream"), content: <section>
      {chunks.length > 0 ? <>
        <div className={styles.meta}>{t("calls.chunks")}: {chunks.length} · {t("calls.bytes")}: {responseBytes}</div>
        <JsonEditor ariaLabel={t("calls.response_stream")} value={responseBody} readOnly detail />
      </> : <div className={styles.empty}>{t("calls.response_content_was_not_recorde")}</div>}
    </section> },
  ];

  if (cursorTrace) tabs.push({ value: "cursor-trace", label: t("calls.cursor_tracing"), content: <section>
      <div className={styles.meta}>
        Request ID: {cursorTrace.trace.request_id} · {t("calls.artifacts")}: {cursorTrace.artifacts.length}
      </div>
      <JsonEditor
        ariaLabel={t("calls.cursor_tracing")}
        value={JSON.stringify({ trace: cursorTrace.trace, artifacts: cursorTrace.artifacts })}
        readOnly
        detail
      />
    </section> });

  return <div className={styles.root}><Tabs items={tabs} /></div>;
}
