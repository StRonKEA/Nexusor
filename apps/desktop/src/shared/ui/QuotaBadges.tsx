import type { PluginResourceMetric } from "../api";
import styles from "./QuotaBadges.module.scss";

type QuotaBadgesProps = {
  metrics: PluginResourceMetric[];
  singleColumnThreshold?: number;
};

export function QuotaBadges({ metrics, singleColumnThreshold = 1 }: QuotaBadgesProps) {
  if (!metrics || metrics.length === 0) return null;

  const relevant = metrics.filter(
    (m) =>
      m.id === "claude-quota" ||
      m.id === "claude-weekly-quota" ||
      m.id === "gemini-quota" ||
      m.id === "gemini-weekly-quota" ||
      m.id === "primary-quota" ||
      m.id === "secondary-quota" ||
      m.id === "copilot-status" ||
      m.id === "credits" ||
      m.id === "kimi-5h" ||
      m.id === "kimi-weekly" ||
      m.id === "claude-code-5h" ||
      m.id === "claude-code-weekly" ||
      m.id === "claude-code-sonnet"
  );

  if (relevant.length === 0) return null;

  return (
    <div
      className={styles.quotaBadgesContainer}
      data-single={relevant.length <= singleColumnThreshold}
    >
      {relevant.map((m) => {
        let badgeLabel = t("models.quota_default_label");
        let fullName = t("models.quota_default_fullname");
        let fullPeriod = t("models.quota_default_label");
        let fillColor = "#3186FF";

        if (m.id === "claude-quota") {
          badgeLabel = t("models.quota_badge_claude_5h");
          fullName = "Claude";
          fullPeriod = t("models.quota_period_5h_session");
          fillColor = m.value > 20 ? "#D97757" : "#ef4444";
        } else if (m.id === "claude-weekly-quota") {
          badgeLabel = t("models.quota_badge_claude_weekly");
          fullName = "Claude";
          fullPeriod = t("models.quota_period_weekly_total");
          fillColor = m.value > 20 ? "#D97757" : "#ef4444";
        } else if (m.id === "gemini-quota") {
          badgeLabel = t("models.quota_badge_gemini_5h");
          fullName = "Gemini";
          fullPeriod = t("models.quota_period_5h_session");
          fillColor = m.value > 20 ? "#3186FF" : "#ef4444";
        } else if (m.id === "gemini-weekly-quota") {
          badgeLabel = t("models.quota_badge_gemini_weekly");
          fullName = "Gemini";
          fullPeriod = t("models.quota_period_weekly_total");
          fillColor = m.value > 20 ? "#3186FF" : "#ef4444";
        } else if (m.id === "primary-quota") {
          badgeLabel = t("models.quota_badge_codex_5h");
          fullName = "OpenAI Codex";
          fullPeriod = t("models.quota_period_5h_session");
          fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
        } else if (m.id === "secondary-quota") {
          badgeLabel = t("models.quota_badge_codex_weekly");
          fullName = "OpenAI Codex";
          fullPeriod = t("models.quota_period_weekly_total");
          fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
        } else if (m.id === "copilot-status") {
          badgeLabel = "Copilot";
          fullName = "GitHub Copilot";
          fullPeriod = t("models.quota_period_active_subscription");
          fillColor = "#238636";
        } else if (m.id === "credits") {
          badgeLabel = t("models.quota_badge_grok_weekly");
          fullName = "xAI Grok";
          fullPeriod = t("models.quota_period_weekly_credits");
          fillColor = m.value > 20 ? "#e0e0e0" : "#ef4444";
        } else if (m.id === "kimi-5h") {
          badgeLabel = t("models.quota_badge_kimi_5h");
          fullName = "Moonshot Kimi";
          fullPeriod = t("models.quota_period_5h_session");
          fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
        } else if (m.id === "kimi-weekly") {
          badgeLabel = t("models.quota_badge_kimi_weekly");
          fullName = "Moonshot Kimi";
          fullPeriod = t("models.quota_period_weekly_total");
          fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
        } else if (m.id === "claude-code-5h") {
          badgeLabel = t("models.quota_badge_claude_code_5h");
          fullName = "Claude Code";
          fullPeriod = t("models.quota_period_5h_session");
          fillColor = m.value > 20 ? "#D97757" : "#ef4444";
        } else if (m.id === "claude-code-weekly") {
          badgeLabel = t("models.quota_badge_claude_code_weekly");
          fullName = "Claude Code";
          fullPeriod = t("models.quota_period_weekly_total");
          fillColor = m.value > 20 ? "#D97757" : "#ef4444";
        } else if (m.id === "claude-code-sonnet") {
          badgeLabel = t("models.quota_badge_sonnet_weekly");
          fullName = "Claude Code Sonnet";
          fullPeriod = t("models.quota_period_weekly_special");
          fillColor = m.value > 20 ? "#D97757" : "#ef4444";
        }

        const percent = Math.round(Math.max(0, Math.min(100, m.value)));
        let tooltip =
          m.id === "copilot-status"
            ? t("models.quota_tooltip_copilot_active")
            : t("models.quota_tooltip_remaining", { fullName, fullPeriod, percent });

        if (m.resetAtMs) {
          const date = new Date(m.resetAtMs);
          const now = Date.now();
          const isFutureDays = m.resetAtMs - now > 86_400_000;
          const timeStr = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
          const dateStr = isFutureDays
            ? `${date.toLocaleDateString([], { month: "short", day: "numeric" })} ${timeStr}`
            : timeStr;
          tooltip += ` · ${t("models.quota_tooltip_reset", { dateStr })}`;
        }

        return (
          <div key={m.id} className={styles.quotaItem} title={tooltip}>
            <span className={styles.quotaLabel}>{badgeLabel}:</span>
            <div className={styles.quotaTrack}>
              <div
                className={styles.quotaFill}
                style={{ width: `${percent}%`, backgroundColor: fillColor }}
              />
            </div>
            <span className={styles.quotaValue}>%{percent}</span>
          </div>
        );
      })}
    </div>
  );
}
