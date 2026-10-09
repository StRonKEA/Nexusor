import { useState } from "react";
import type { ModelConnectivityResult } from "../../shared/api";
import { Icon } from "../../shared/ui/Icon";
import { TooltipTrigger } from "../../shared/ui/TooltipTrigger";
import { closeCircleIcon, informationOutlineIcon } from "../../shared/ui/icons";
import styles from "./CursorModelTestResult.module.scss";

export type CursorModelTestState =
  | { status: "success"; result: ModelConnectivityResult }
  | { status: "error"; error: string }
  | { status: "cancelled" };

/** Hata metnini kısalt: ilk anlamlı cümle + "..." */
function shortenError(raw: string): { short: string; full: string; truncated: boolean } {
  // "provider error: " prefix'ini temizle
  let msg = raw.trim();
  while (msg.startsWith("provider error: ")) {
    msg = msg.slice("provider error: ".length).trim();
  }

  // HTTP body JSON'unu ayır
  const jsonStart = msg.indexOf("{");
  const prefix = jsonStart > 0 ? msg.slice(0, jsonStart).trim() : msg;

  // Anlamlı kısa metin: ilk iki nokta'ya kadar (veya 100 karakter)
  const sentences = prefix.replace(/\n/g, " ").split(/(?<=\.)\s+/);
  const short = sentences.slice(0, 2).join(" ").slice(0, 120);
  const truncated = msg.length > short.length + 5;

  return { short: short || msg.slice(0, 120), full: msg, truncated };
}

export function CursorModelTestResult({ state, testing = false, compact = false }: {
  state?: CursorModelTestState;
  testing?: boolean;
  /** Listede gösterilen küçük rozet biçimi */
  compact?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);

  if (testing) {
    return compact
      ? <span className={`${styles.compact} ${styles.testing}`}>{t("models.testing")}</span>
      : <div className={`${styles.root} ${styles.testing}`}><span className={styles.summary}>{t("models.testing")}</span></div>;
  }
  if (!state) {
    return compact ? null : <div className={`${styles.root} ${styles.idle}`}><span className={styles.summary}>{t("models.not_tested")}</span></div>;
  }
  if (state.status === "cancelled") {
    return compact
      ? <span className={`${styles.compact} ${styles.idle}`}>{t("models.test_cancelled")}</span>
      : <div className={`${styles.root} ${styles.idle}`}><span className={styles.summary}>{t("models.test_cancelled")}</span></div>;
  }

  const success = state.status === "success";

  if (compact) {
    const detail = success
      ? t("models.speed_tokens_s_first_token_ms_to", {
        speed: formatSpeed(state.result.tokens_per_second),
        firstText: state.result.first_valid_response_ms ?? "--",
        duration: state.result.duration_ms,
        tokens: state.result.output_tokens,
        estimated: state.result.tokens_estimated ? t("models.estimated") : "",
        output: state.result.output || "--",
      })
      : state.error;
    return (
      <TooltipTrigger label={detail}>
        <span className={`${styles.compact} ${success ? styles.success : styles.error}`}>
          {success ? `${formatSpeed(state.result.tokens_per_second)} tokens/s` : t("models.test_failed_2")}
          <Icon icon={informationOutlineIcon} size="1em" />
        </span>
      </TooltipTrigger>
    );
  }

  // ── Full (banner) mode ───────────────────────────────────────────────────
  if (success) {
    const summary = t("models.speed_tokens_s", { speed: formatSpeed(state.result.tokens_per_second) });
    const detail = t("models.speed_tokens_s_first_token_ms_to", {
      speed: formatSpeed(state.result.tokens_per_second),
      firstText: state.result.first_valid_response_ms ?? "--",
      duration: state.result.duration_ms,
      tokens: state.result.output_tokens,
      estimated: state.result.tokens_estimated ? t("models.estimated") : "",
      output: state.result.output || "--",
    });
    return (
      <div className={`${styles.root} ${styles.success}`}>
        <span className={styles.summary}>{summary}</span>
        <TooltipTrigger label={detail}>
          <button type="button" className={styles.details}>
            {t("calls.view_details")}<Icon icon={informationOutlineIcon} size="1.1em" />
          </button>
        </TooltipTrigger>
      </div>
    );
  }

  // ── Error mode ──────────────────────────────────────────────────────────
  const { short, full, truncated } = shortenError(state.error);

  return (
    <div className={`${styles.root} ${styles.error}`}>
      <div className={styles.errorBody}>
        <span className={styles.errorIcon}><Icon icon={closeCircleIcon} size="1.2em" /></span>
        <div className={styles.errorContent}>
          <span className={styles.errorShort}>{short}{truncated && !expanded ? "…" : ""}</span>
          {truncated && expanded && (
            <pre className={styles.errorFull}>{full}</pre>
          )}
          {truncated && (
            <button
              type="button"
              className={styles.expandBtn}
              onClick={() => setExpanded((v) => !v)}
            >
              {expanded ? t("models.collapse_details") : t("models.show_full_error")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function formatSpeed(value: number) {
  return Number.isFinite(value) ? value.toFixed(1) : "0.0";
}
