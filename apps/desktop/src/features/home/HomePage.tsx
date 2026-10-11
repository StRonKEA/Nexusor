import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { api, type LlmCall, type Overview, type PluginResourceMetric } from "../../shared/api";
import { DailyTokenUsageChart } from "./charts/DailyTokenUsageChart";
import { PageContent } from "../../shell/layout/PageContent";
import { appStore, useAppStore } from "../../shared/store/appStore";
import { useI18n } from "../../i18n/store";
import { Icon } from "../../shared/ui/Icon";
import { checkIcon, clockIcon, chevronDownIcon, closeIcon, alertCircleIcon, providerIcon } from "../../shared/ui/icons";
import { getProviderLogo } from "../../shared/utils/providerIcons";
import { formatCompactInteger } from "../../shared/utils/numberFormat";
import { QuotaBadges } from "../../shared/ui/QuotaBadges";
import styles from "./HomePage.module.scss";

export function HomePage() {
  const { overview, cursorHarness, plugins } = useAppStore();
  const { locale } = useI18n();
  const navigate = useNavigate();
  const [weekOverview, setWeekOverview] = useState<Overview | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [latestCall, setLatestCall] = useState<LlmCall | null>(null);

  // Poll latest call for the live traffic indicator
  // Combined with the store refresh into a single unified polling cycle.
  useEffect(() => {
    let active = true;
    const refreshAll = () => {
      if (document.visibilityState !== "visible") return;
      void appStore.refresh();
      void api.calls().then((res) => {
        if (active && res.length > 0) {
          setLatestCall(res[0]);
        }
      }).catch(() => {});
    };
    refreshAll();
    const timer = setInterval(refreshAll, 10_000);
    window.addEventListener("focus", refreshAll);
    return () => {
      active = false;
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

  // 1. Cursor Status Metrics
  const isTakenOver = cursorHarness?.settings_applied ?? false;
  const caReady = cursorHarness?.ca === "ready";
  const proxyPort = cursorHarness?.proxy_url
    ? cursorHarness.proxy_url.split(":").pop() || "6332"
    : "—";

  // 2. Account Pool Metrics
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

  const readyAccounts = allAccounts.filter((a) => a.state.status === "ready");
  const coolingAccounts = allAccounts.filter((a) => a.state.status === "cooling");
  const disabledAccounts = allAccounts.filter((a) => a.state.status === "disabled");

  const [dismissedAlertIds, setDismissedAlertIds] = useState<Set<string>>(() => new Set());
  const [alertsExpanded, setAlertsExpanded] = useState<boolean>(false);

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

      // Proaktif Kota Kontrolü: Kotası kritik (%15 altı) veya tükenmiş hesaplar için erken uyarı
      for (const m of a.metrics) {
        if (m.id.includes("quota") || m.id.includes("credits")) {
          if (m.value > 0 && m.value <= 15) {
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

  // Master Traffic-Light Status Determination
  const masterState: "ok" | "warn" | "error" = useMemo(() => {
    if (!isTakenOver || allAccounts.length === 0) return "error";
    if (coolingAccounts.length > 0 || activeAlerts.length > 0) return "warn";
    return "ok";
  }, [isTakenOver, allAccounts.length, coolingAccounts.length, activeAlerts.length]);

  const dashboardContent = (
    <div className={styles.dashboard}>
      {/* ── 0. Master Traffic-Light Status Card (Item 1) ── */}
      <div className={styles.masterStatusCard} data-state={masterState}>
        <div className={styles.masterStatusLeft}>
          <div className={styles.masterStatusDot} data-state={masterState} />
          <div className={styles.masterStatusTextGroup}>
            <h4 className={styles.masterStatusTitle}>
              {allAccounts.length === 0
                ? t("home.status_no_accounts_title")
                : !isTakenOver
                ? t("home.status_disconnected_title")
                : masterState === "warn"
                ? t("home.status_degraded_title")
                : t("home.status_all_operational_title")}
            </h4>
            <p className={styles.masterStatusDesc}>
              {allAccounts.length === 0
                ? t("home.status_no_accounts_desc")
                : !isTakenOver
                ? t("home.status_disconnected_desc")
                : masterState === "warn"
                ? t("home.status_degraded_desc")
                : t("home.status_all_operational_desc")}
            </p>
          </div>
        </div>

        {(!isTakenOver || allAccounts.length === 0) && (
          <div className={styles.masterStatusAction}>
            {!isTakenOver ? (
              <button
                type="button"
                className={styles.masterStatusBtn}
                onClick={() => navigate("/cursor")}
              >
                {t("home.btn_repair_cursor")}
              </button>
            ) : (
              <button
                type="button"
                className={styles.masterStatusBtn}
                onClick={() => navigate("/providers")}
              >
                {t("home.btn_connect_account")}
              </button>
            )}
          </div>
        )}
      </div>

      {/* ── Empty State Onboarding: When no accounts are connected (Item 6) ── */}
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

      {/* ── 2. Live Providers & Quotas (Grouped by Provider, with Account Pool Summary on the Right!) ── */}
      {allAccounts.length > 0 && (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <div className={styles.sectionHeader}>
            <h3 className={styles.sectionTitle}>{t("home.live_providers_title")}</h3>
            <div className={styles.poolSummaryHeader}>
              <span className={styles.poolTotalBadge}>
                {t("home.connected_accounts_count", { count: allAccounts.length })}
              </span>
              <span className={styles.metaItemReady}>
                <span className={styles.statusDot} />
                <span>{t("home.status_ready_count", { count: readyAccounts.length })}</span>
              </span>
              {coolingAccounts.length > 0 && (
                <span className={styles.metaItemWarn}>
                  <span className={styles.statusDot} />
                  <span>{t("home.status_cooling_count", { count: coolingAccounts.length })}</span>
                </span>
              )}
              {disabledAccounts.length > 0 && (
                <span className={styles.metaItemDisabled}>
                  <span className={styles.statusDot} />
                  <span>{t("home.status_disabled_count", { count: disabledAccounts.length })}</span>
                </span>
              )}
            </div>
          </div>

          <div className={styles.providersStrip}>
            {plugins
              .filter((p) => p.resources.some((r) => r.resources.length > 0))
              .map((p) => {
                const accounts = p.resources.flatMap((r) => r.resources);
                const firstAccount = accounts.find((a) => a.state.status === "ready") || accounts[0];
                const logo = getProviderLogo(p.id) || p.icon;

                return (
                  <div
                    key={p.id}
                    className={styles.providerSummaryCard}
                    role="button"
                    tabIndex={0}
                    onClick={() => navigate("/models")}
                    onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") navigate("/models"); }}
                    style={{ cursor: "pointer" }}
                  >
                    <div className={styles.providerLeft}>
                      {logo && <img src={logo} alt="" className={styles.providerLogo} />}
                      <span className={styles.providerName}>{p.name}</span>
                      <span className={styles.providerAccountsBadge}>
                        {t("home.accounts_count", { count: accounts.length })}
                      </span>
                    </div>

                    <div className={styles.providerRight}>
                      {firstAccount && <QuotaBadges metrics={firstAccount.metrics} />}
                    </div>
                  </div>
                );
              })}
          </div>
        </div>
      )}

      {/* ── 3. Daily Token Usage Chart (Last 7 Days) ── */}
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
      {latestCall && (
        <Link
          to="/calls"
          className={styles.liveTrafficBadge}
          title={t("home.live_traffic_tooltip")}
        >
          <span
            className={styles.liveTrafficDot}
            style={{
              background: latestCall.status === "completed" ? "#27c376" : latestCall.status === "running" ? "#3186FF" : "#ef4444",
            }}
          />
          <span>
            {latestCall.display_name || latestCall.model_id}
            {latestCall.duration_ms ? ` (${latestCall.duration_ms}ms)` : ""}
          </span>
        </Link>
      )}
      <Link
        to="/cursor"
        className={styles.titleCursorBadge}
        data-taken={isTakenOver ? "true" : "false"}
        title={t("nav.cursor_integration")}
      >
        <span className={isTakenOver ? styles.titleCursorDotOk : styles.titleCursorDotOff} />
        <span>Cursor: {isTakenOver ? t("cursor.taken_over") : t("cursor.not_taken_over")}</span>
        <span className={styles.titleCursorSub}>:{proxyPort}</span>
        {caReady && <Icon icon={checkIcon} size="1.1em" className={styles.titleCheckIcon} />}
      </Link>
    </div>
  );

  return (
    <PageContent
      title={pageTitle}
      sections={[{ key: "dashboard", estimatedHeight: 700, content: dashboardContent }]}
    />
  );
}
