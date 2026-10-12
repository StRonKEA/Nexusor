import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { api, type Overview } from "../../shared/api";
import { DailyTokenUsageChart } from "./charts/DailyTokenUsageChart";
import { PageContent } from "../../shell/layout/PageContent";
import { appStore, useAppStore } from "../../shared/store/appStore";
import { useI18n } from "../../i18n/store";
import { Icon } from "../../shared/ui/Icon";
import { clockIcon, chevronDownIcon, closeIcon, providerIcon } from "../../shared/ui/icons";
import { formatCompactInteger } from "../../shared/utils/numberFormat";
import styles from "./HomePage.module.scss";

export function HomePage() {
  const { overview, plugins } = useAppStore();
  const { locale: _locale } = useI18n();
  const navigate = useNavigate();
  const [weekOverview, setWeekOverview] = useState<Overview | null>(null);
  const now = Date.now();

  // Unified store refresh cycle
  useEffect(() => {
    const refreshAll = () => {
      if (document.visibilityState !== "visible") return;
      void appStore.refresh();
    };
    refreshAll();
    const timer = setInterval(refreshAll, 10_000);
    window.addEventListener("focus", refreshAll);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", refreshAll);
    };
  }, []);

  // Fetch last 7 days overview for the compact chart
  useEffect(() => {
    let active = true;
    const fetchWeekOverview = () => {
      const now = Date.now();
      const startMs = now - 7 * 24 * 60 * 60_000;
      void api
        .overview({ startMs, endMs: now })
        .then((res) => {
          if (active) setWeekOverview(res);
        })
        .catch(() => {});
    };

    fetchWeekOverview();
    return () => {
      active = false;
    };
  }, [overview?.token_usage_series?.length]);

  const activeOverview = weekOverview ?? overview;

  const allAccounts = useMemo(() => {
    return plugins.flatMap((p) =>
      p.resources.flatMap((r) =>
        r.resources.map((item) => ({
          ...item,
          pluginId: p.id,
          pluginName: p.name,
          resourceType: r.type,
        }))
      )
    );
  }, [plugins]);

  const [dismissedAlertIds, setDismissedAlertIds] = useState<Set<string>>(() => new Set());
  const [alertsExpanded, setAlertsExpanded] = useState<boolean>(false);

  // Read custom quota threshold from settings (fallback to 15%)
  const quotaThreshold = useMemo(() => {
    const raw = localStorage.getItem("nexusor_quota_alert_threshold");
    const num = Number(raw);
    return Number.isFinite(num) && num > 0 ? num : 15;
  }, []);

  const notificationsEnabled = useMemo(() => {
    return localStorage.getItem("nexusor_quota_notifications_enabled") !== "false";
  }, []);

  const coolingOrExhaustedItems = useMemo(() => {
    const list: Array<{
      id: string;
      type: "cooling" | "low_quota";
      provider: string;
      account?: string | null;
      model?: string | null;
      percent?: number;
      retryAtMs: number | null;
    }> = [];

    for (const a of allAccounts) {
      const stateRetryAt =
        a.state.retryAtMs ?? (a.state as { retry_at_ms?: number | null }).retry_at_ms ?? null;

      // 1. Account is explicitly in Cooling state
      if (a.state.status === "cooling") {
        list.push({
          id: a.id,
          type: "cooling",
          provider: a.pluginName,
          account: a.displayName || null,
          model: null,
          retryAtMs: stateRetryAt,
        });
        continue;
      }

      // Proaktif Kota Kontrolü: Kotası kullanıcının belirlediği eşik altı olan hesaplar için erken uyarı
      if (notificationsEnabled) {
        for (const m of a.metrics) {
          if (m.id.includes("quota") || m.id.includes("credits")) {
            if (m.value > 0 && m.value <= quotaThreshold) {
              list.push({
                id: `${a.id}-low-${m.id}`,
                type: "low_quota",
                provider: a.pluginName,
                account: a.displayName || a.id,
                model: null,
                percent: Math.round(m.value),
                retryAtMs: m.resetAtMs ?? null,
              });
            }
          }
        }
      }

      // 2. Antigravity per-model quota checks (Gemini vs Claude)
      if (a.pluginId.includes("antigravity")) {
        const gemini5h = a.metrics.find((m) => m.id === "gemini-quota");
        const geminiWk = a.metrics.find((m) => m.id === "gemini-weekly-quota");
        const claude5h = a.metrics.find((m) => m.id === "claude-quota");
        const claudeWk = a.metrics.find((m) => m.id === "claude-weekly-quota");

        const gExhausted =
          (gemini5h && gemini5h.value <= 0) || (geminiWk && geminiWk.value <= 1);
        const cExhausted =
          (claude5h && claude5h.value <= 0) || (claudeWk && claudeWk.value <= 1);

        if (gExhausted) {
          const resetMs =
            geminiWk && geminiWk.value <= 1
              ? (geminiWk.resetAtMs ?? null)
              : (gemini5h?.resetAtMs ?? null);
          list.push({
            id: `${a.id}-gemini`,
            type: "cooling",
            provider: a.pluginName,
            account: a.displayName || a.id,
            model: "Gemini",
            retryAtMs: resetMs,
          });
        }

        if (cExhausted) {
          const resetMs =
            claudeWk && claudeWk.value <= 1
              ? (claudeWk.resetAtMs ?? null)
              : (claude5h?.resetAtMs ?? null);
          list.push({
            id: `${a.id}-claude`,
            type: "cooling",
            provider: a.pluginName,
            account: a.displayName || a.id,
            model: "Claude",
            retryAtMs: resetMs,
          });
        }
      }

      // 3. Grok credits check
      if (a.pluginId.includes("grok")) {
        const credits = a.metrics.find((m) => m.id === "credits");
        if (credits && credits.value <= 0) {
          list.push({
            id: a.id,
            type: "cooling",
            provider: a.pluginName,
            account: a.displayName || null,
            model: null,
            retryAtMs: credits.resetAtMs ?? stateRetryAt,
          });
        }
      }

      // 4. Codex / ChatGPT quota check
      if (a.pluginId.includes("codex")) {
        const prim = a.metrics.find((m) => m.id === "primary-quota");
        const sec = a.metrics.find((m) => m.id === "secondary-quota");
        if ((prim && prim.value <= 0) || (sec && sec.value <= 0)) {
          list.push({
            id: a.id,
            type: "cooling",
            provider: a.pluginName,
            account: a.displayName || null,
            model: null,
            retryAtMs: (prim && prim.value <= 0 ? prim.resetAtMs : sec?.resetAtMs) ?? null,
          });
        }
      }
    }

    return list;
  }, [allAccounts]);

  // Filter out alerts explicitly dismissed by the user in this session
  const activeAlerts = useMemo(() => {
    return coolingOrExhaustedItems.filter((item) => !dismissedAlertIds.has(item.id));
  }, [coolingOrExhaustedItems, dismissedAlertIds]);

  const handleDismissAlert = (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    setDismissedAlertIds((prev) => new Set([...prev, id]));
  };

  const handleDismissAllAlerts = (e: React.MouseEvent) => {
    e.stopPropagation();
    setDismissedAlertIds(new Set(coolingOrExhaustedItems.map((item) => item.id)));
  };

  const formatRemainingDuration = (retryAtMs?: number | null) => {
    if (!retryAtMs || retryAtMs <= now) return t("home.duration_seconds", { seconds: 0 });
    const diffMs = retryAtMs - now;
    const totalSecs = Math.floor(diffMs / 1000);
    const hours = Math.floor(totalSecs / 3600);
    const minutes = Math.floor((totalSecs % 3600) / 60);
    const seconds = totalSecs % 60;

    if (hours > 0) {
      return minutes > 0
        ? t("home.duration_hours_minutes", { hours, minutes })
        : t("home.duration_hours", { hours });
    }
    if (minutes > 0) {
      return seconds > 0
        ? t("home.duration_minutes_seconds", { minutes, seconds })
        : t("home.duration_minutes", { minutes });
    }
    return t("home.duration_seconds", { seconds });
  };

  // 3. Usage & Metrics
  const metrics = activeOverview.metrics;
  const totalCalls = metrics.llm_calls;
  const failedCalls = metrics.failed_calls;
  const successCalls = metrics.successful_calls;
  const successRate = totalCalls > 0 ? Math.round((successCalls / totalCalls) * 100) : 100;
  const netInput = metrics.input_tokens;
  const output = metrics.output_tokens;
  const netTotal = netInput + output;
  const cachedTokens = metrics.cache_read_tokens;

  const chartData = useMemo(() => {
    return activeOverview.token_usage_series.map((bucket) => ({
      bucketStartMs: bucket.bucket_start_ms,
      inputTokens: bucket.input_tokens,
      cacheReadTokens: bucket.cache_read_tokens,
      cacheWriteTokens: bucket.cache_write_tokens,
      outputTokens: bucket.output_tokens,
    }));
  }, [activeOverview]);

  const dashboardContent = (
    <div className={styles.dashboard}>
      {/* ── Empty State Onboarding: When no accounts are connected ── */}
      {allAccounts.length === 0 ? (
        <div className={styles.onboardingCard}>
          <div className={styles.onboardingIcon}>
            <Icon icon={providerIcon} />
          </div>
          <h3 className={styles.onboardingTitle}>{t("home.empty_onboarding_title")}</h3>
          <p className={styles.onboardingDesc}>{t("home.empty_onboarding_desc")}</p>
          <button
            type="button"
            className={styles.onboardingBtn}
            onClick={() => navigate("/providers")}
          >
            {t("home.empty_onboarding_action")}
          </button>
        </div>
      ) : (
        <>
          {/* ── 1. Hero KPI Grid (3 Balanced, Equal Metric Cards) ── */}
          <div className={styles.heroGrid3}>
            {/* Card 1: Token Usage */}
            <Link to="/calls" className={styles.heroCardKpiLink}>
              <div className={styles.heroCardKpi}>
                <div className={styles.heroCardHeader}>
                  <span className={styles.heroCardTitle}>{t("home.usage_metrics_title")}</span>
                  <span className={styles.statusBadgeOk}>{t("home.token_volume")}</span>
                </div>
                <div className={styles.heroValue}>
                  {formatCompactInteger(netTotal)} {t("home.token_unit")}
                </div>
                <div className={styles.heroSplitRow}>
                  <span>{t("home.stat_input")}: <strong className={styles.heroSplitBadge}>{formatCompactInteger(netInput)}</strong></span>
                  <span>·</span>
                  <span>{t("home.stat_output")}: <strong className={styles.heroSplitBadge}>{formatCompactInteger(output)}</strong></span>
                </div>
              </div>
            </Link>

            {/* Card 2: Cache Savings */}
            <Link to="/calls" className={styles.heroCardKpiLink}>
              <div className={styles.heroCardKpi}>
                <div className={styles.heroCardHeader}>
                  <span className={styles.heroCardTitle}>{t("home.cache_title")}</span>
                  <span className={styles.statusBadgeOk}>
                    %{Math.round((cachedTokens / Math.max(1, metrics.prompt_tokens)) * 100)} {t("home.cache_rate")}
                  </span>
                </div>
                <div className={styles.heroValue}>
                  {formatCompactInteger(cachedTokens)} {t("home.token_unit")}
                </div>
                <div className={styles.cacheMeterBar}>
                  <div
                    className={styles.cacheMeterFill}
                    style={{
                      width: `${Math.min(
                        100,
                        Math.round((cachedTokens / Math.max(1, metrics.prompt_tokens)) * 100)
                      )}%`,
                    }}
                  />
                </div>
                <div className={styles.heroMeta} style={{ marginTop: 6 }}>
                  <span>{t("home.cache_description", { total: formatCompactInteger(metrics.prompt_tokens), cached: formatCompactInteger(cachedTokens) })}</span>
                </div>
              </div>
            </Link>

            {/* Card 3: Calls & Success Rate */}
            <Link to="/calls" className={styles.heroCardKpiLink}>
              <div className={styles.heroCardKpi}>
                <div className={styles.heroCardHeader}>
                  <span className={styles.heroCardTitle}>{t("home.calls_health_title")}</span>
                  <span className={styles.statusBadgeOk}>%{successRate} {t("home.success_label")}</span>
                </div>
                <div className={styles.heroValue}>
                  {t("home.total_calls", { count: totalCalls })}
                </div>
                <div className={styles.heroMeta}>
                  {failedCalls > 0 ? (
                    <span style={{ color: "#ef4444", fontWeight: 600 }}>{t("home.errors_count", { count: failedCalls })}</span>
                  ) : (
                    <span style={{ color: "#27c376", fontWeight: 600 }}>{t("home.zero_errors")}</span>
                  )}
                  <span>·</span>
                  <span>{t("home.continuous_flow")}</span>
                </div>
              </div>
            </Link>
          </div>
        </>
      )}

      {/* ── Optional Alert Banners: Live Cooling Down & Low Quota Accounts ── */}
      {activeAlerts.length > 0 && (
        <div className={styles.coolingAlertContainer}>
          {/* If multiple alerts exist, provide a compact collapsible summary banner with dismiss-all */}
          {activeAlerts.length > 1 && (
            <div className={styles.alertsSummaryBanner}>
              <div
                className={styles.alertsSummaryLeft}
                role="button"
                tabIndex={0}
                onClick={() => setAlertsExpanded((prev) => !prev)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    setAlertsExpanded((prev) => !prev);
                  }
                }}
              >
                <div className={styles.coolingAlertIcon}>
                  <Icon icon={clockIcon} />
                </div>
                <span className={styles.alertsSummaryTitle}>
                  {t("home.alerts_summary_title", { count: activeAlerts.length })}
                </span>
              </div>
              <div className={styles.alertsSummaryActions}>
                <button
                  type="button"
                  className={styles.alertsToggleBtn}
                  onClick={() => setAlertsExpanded((prev) => !prev)}
                >
                  <Icon
                    icon={chevronDownIcon}
                    style={{
                      transform: alertsExpanded ? "rotate(180deg)" : "none",
                      transition: "transform 0.2s ease",
                    }}
                  />
                  <span>
                    {alertsExpanded ? t("home.alerts_collapse") : t("home.alerts_show_all")}
                  </span>
                </button>
                <button
                  type="button"
                  className={styles.alertsDismissAllBtn}
                  onClick={handleDismissAllAlerts}
                  title={t("home.alerts_dismiss_all")}
                >
                  <Icon icon={closeIcon} />
                  <span>{t("home.alerts_dismiss_all")}</span>
                </button>
              </div>
            </div>
          )}

          {/* Show full list if expanded OR if there is only 1 alert */}
          {(alertsExpanded || activeAlerts.length === 1) && (
            <div className={styles.alertsListCollapsible}>
              {activeAlerts.map((item) => {
                let noticeText = "";

                if (item.type === "low_quota") {
                  if (item.account && item.model) {
                    noticeText = t("home.low_quota_notice_account_model", {
                      provider: item.provider,
                      account: item.account,
                      model: item.model,
                      percent: item.percent ?? 0,
                    });
                  } else if (item.account) {
                    noticeText = t("home.low_quota_notice_account", {
                      provider: item.provider,
                      account: item.account,
                      percent: item.percent ?? 0,
                    });
                  } else {
                    noticeText = t("home.low_quota_notice_account", {
                      provider: item.provider,
                      account: item.provider,
                      percent: item.percent ?? 0,
                    });
                  }
                } else {
                  // Cooling state
                  const remainingStr = formatRemainingDuration(item.retryAtMs);
                  if (item.account && item.model) {
                    noticeText = t("home.cooling_notice_account_model", {
                      provider: item.provider,
                      account: item.account,
                      model: item.model,
                      remaining: remainingStr,
                    });
                  } else if (item.account) {
                    noticeText = t("home.cooling_notice_account", {
                      provider: item.provider,
                      account: item.account,
                      remaining: remainingStr,
                    });
                  } else {
                    noticeText = t("home.cooling_notice", {
                      provider: item.provider,
                      remaining: remainingStr,
                    });
                  }
                }

                return (
                  <div
                    key={item.id}
                    className={styles.coolingAlertBanner}
                    role="button"
                    tabIndex={0}
                    onClick={() => navigate("/models")}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") navigate("/models");
                    }}
                    title={t("home.go_to_models")}
                  >
                    <div className={styles.coolingAlertIcon}>
                      <Icon icon={clockIcon} />
                    </div>
                    <div className={styles.coolingAlertText}>{noticeText}</div>
                    <button
                      type="button"
                      className={styles.alertDismissBtn}
                      onClick={(e) => handleDismissAlert(e, item.id)}
                      title={t("home.alerts_dismiss")}
                    >
                      <Icon icon={closeIcon} />
                    </button>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {/* ── Daily Token Usage Chart (Last 7 Days) ── */}
      <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 4 }}>
        <div className={styles.sectionHeader}>
          <h3 className={styles.sectionTitle}>{t("home.token_trend_title")}</h3>
        </div>
        <div className={styles.chartCard}>
          <DailyTokenUsageChart
            data={chartData}
            granularity={activeOverview.token_usage_granularity}
          />
        </div>
      </div>
    </div>
  );

  const pageTitle = (
    <div className={styles.pageTitleRow}>
      <span>{t("home.summary")}</span>
    </div>
  );

  return (
    <PageContent
      title={pageTitle}
      sections={[{ key: "dashboard", estimatedHeight: 700, content: dashboardContent }]}
    />
  );
}
