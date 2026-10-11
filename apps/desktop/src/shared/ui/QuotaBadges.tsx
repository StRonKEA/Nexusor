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
        let badgeLabel = t("home.badge_quota");
        let fullName = t("home.badge_model");
        let fullPeriod = t("home.badge_quota");
        let fillColor = "#3186FF";

        if (m.id === "claude-quota") {
          badgeLabel = t("home.badge_claude_5h");
          fullName = "Claude";
          fullPeriod = t("home.badge_period_5h");
          fillColor = m.value > 20 ? "#D97757" : "#ef4444";
        } else if (m.id === "claude-weekly-quota") {
          badgeLabel = t("home.badge_claude_week");
          fullName = "Claude";
          fullPeriod = t("home.badge_period_weekly");
          fillColor = m.value > 20 ? "#D97757" : "#ef4444";
        } else if (m.id === "gemini-quota") {
          badgeLabel = t("home.badge_gemini_5h");
          fullName = "Gemini";
          fullPeriod = t("home.badge_period_5h");
          fillColor = m.value > 20 ? "#3186FF" : "#ef4444";
        } else if (m.id === "gemini-weekly-quota") {
          badgeLabel = t("home.badge_gemini_week");
          fullName = "Gemini";
          fullPeriod = t("home.badge_period_weekly");
          fillColor = m.value > 20 ? "#3186FF" : "#ef4444";
        } else if (m.id === "primary-quota") {
          badgeLabel = t("home.badge_codex_5h");
          fullName = "OpenAI Codex";
          fullPeriod = t("home.badge_period_5h");
          fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
        } else if (m.id === "secondary-quota") {
          badgeLabel = t("home.badge_codex_week");
          fullName = "OpenAI Codex";
          fullPeriod = t("home.badge_period_weekly");
          fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
        } else if (m.id === "copilot-status") {
          badgeLabel = "Copilot";
          fullName = "GitHub Copilot";
          fullPeriod = t("home.badge_active_subscription");
          fillColor = "#238636";
        } else if (m.id === "credits") {
          badgeLabel = t("home.badge_grok_week");
          fullName = "xAI Grok";
          fullPeriod = t("home.badge_weekly_credits");
          fillColor = m.value > 20 ? "#e0e0e0" : "#ef4444";
        } else if (m.id === "kimi-5h") {
          badgeLabel = t("home.badge_kimi_5h");
          fullName = "Moonshot Kimi";
          fullPeriod = t("home.badge_period_5h");
          fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
        } else if (m.id === "kimi-weekly") {
          badgeLabel = t("home.badge_kimi_week");
          fullName = "Moonshot Kimi";
          fullPeriod = t("home.badge_period_weekly");
          fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
        } else if (m.id === "claude-code-5h") {
          badgeLabel = t("home.badge_claude_code_5h");
          fullName = "Claude Code";
          fullPeriod = t("home.badge_period_5h");
          fillColor = m.value > 20 ? "#D97757" : "#ef4444";
        } else if (m.id === "claude-code-weekly") {
          badgeLabel = t("home.badge_claude_code_week");
          fullName = "Claude Code";
          fullPeriod = t("home.badge_period_weekly");
          fillColor = m.value > 20 ? "#D97757" : "#ef4444";
        } else if (m.id === "claude-code-sonnet") {
          badgeLabel = t("home.badge_claude_sonnet_week");
          fullName = "Claude Code Sonnet";
          fullPeriod = t("home.badge_period_special_weekly");
          fillColor = m.value > 20 ? "#D97757" : "#ef4444";
        }

        const percent = Math.round(Math.max(0, Math.min(100, m.value)));
        let tooltip =
          m.id === "copilot-status"
            ? `${fullName}: ${t("home.badge_active_subscription")} (${t("home.badge_unlimited_usage")})`
            : `${fullName} (${fullPeriod}): %${percent} ${t("home.badge_remaining")}`;

        if (m.resetAtMs) {
          const date = new Date(m.resetAtMs);
          const now = Date.now();
          const isFutureDays = m.resetAtMs - now > 86_400_000;
          const timeStr = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
          const dateStr = isFutureDays
            ? `${date.toLocaleDateString([], { month: "short", day: "numeric" })} ${timeStr}`
            : timeStr;
          tooltip += ` · ${t("home.badge_reset")}: ${dateStr}`;
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
