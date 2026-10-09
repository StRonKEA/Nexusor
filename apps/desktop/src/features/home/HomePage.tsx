import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { api, type LlmCall, type Overview, type PluginResourceMetric } from "../../shared/api";
import { DailyTokenUsageChart } from "./charts/DailyTokenUsageChart";
import { PageContent } from "../../shell/layout/PageContent";
import { appStore, useAppStore } from "../../shared/store/appStore";
import { useI18n } from "../../i18n/store";
import { Icon } from "../../shared/ui/Icon";
import { checkIcon, clockIcon } from "../../shared/ui/icons";
import { getProviderLogo } from "../../shared/utils/providerIcons";
import { formatCompactInteger } from "../../shared/utils/numberFormat";
import styles from "./HomePage.module.scss";

export function HomePage() {
  const { overview, cursorHarness, plugins } = useAppStore();
  const { locale } = useI18n();
  const navigate = useNavigate();
  const [weekOverview, setWeekOverview] = useState<Overview | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [latestCall, setLatestCall] = useState<LlmCall | null>(null);

  // Poll latest call for the live traffic indicator
  useEffect(() => {
    let active = true;
    const fetchLatest = () => {
      void api.calls().then((res) => {
        if (active && res.length > 0) {
          setLatestCall(res[0]);
        }
      }).catch(() => {});
    };
    fetchLatest();
    const timer = setInterval(fetchLatest, 5000);
    return () => {
      active = false;
      clearInterval(timer);
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

  // Periodic background refresh for HomePage so status & quotas update automatically while user waits.
  // Skipped while the window is hidden, matching CallsPage: this keeps a hidden panel
  // from hitting the store every 15 seconds.
  useEffect(() => {
    const refresh = () => {
      if (document.visibilityState === "visible") void appStore.refresh();
    };
    refresh();
    const timer = setInterval(refresh, 15_000);
    window.addEventListener("focus", refresh);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", refresh);
    };
  }, []);

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

  const coolingOrExhaustedItems = useMemo(() => {
    const list: Array<{
      id: string;
      provider: string;
      account?: string | null;
      model?: string | null;
      retryAtMs: number | null;
    }> = [];

    for (const a of allAccounts) {
      const stateRetryAt =
        a.state.retryAtMs ?? (a.state as { retry_at_ms?: number | null }).retry_at_ms ?? null;

      // 1. Account is explicitly in Cooling state
      if (a.state.status === "cooling") {
        list.push({
          id: a.id,
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
              provider: a.pluginName,
              account: a.displayName || a.id,
              model: `Düşük Kota: %${Math.round(m.value)} kaldı (Yedek hesaba hazır)`,
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

  // Live second-by-second countdown for cooling & exhausted accounts
  useEffect(() => {
    if (coolingOrExhaustedItems.length === 0) return;
    const interval = setInterval(() => {
      setNow(Date.now());
    }, 1000);
    return () => clearInterval(interval);
  }, [coolingOrExhaustedItems.length]);

  const formatRemainingDuration = (retryAtMs?: number | null) => {
    if (!retryAtMs || retryAtMs <= now) return locale.startsWith("tr") ? "0 saniye" : "0 seconds";
    const diffMs = retryAtMs - now;
    const totalSecs = Math.floor(diffMs / 1000);
    const hours = Math.floor(totalSecs / 3600);
    const minutes = Math.floor((totalSecs % 3600) / 60);
    const seconds = totalSecs % 60;

    const isTr = locale.startsWith("tr");
    const isZh = locale.startsWith("zh");
    const isPt = locale.startsWith("pt");

    if (hours > 0) {
      if (isTr) return minutes > 0 ? `${hours} saat ${minutes} dakika` : `${hours} saat`;
      if (isZh) return minutes > 0 ? `${hours} 小时 ${minutes} 分钟` : `${hours} 小时`;
      if (isPt) return minutes > 0 ? `${hours} horas e ${minutes} minutos` : `${hours} horas`;
      return minutes > 0 ? `${hours} hours ${minutes} minutes` : `${hours} hours`;
    }
    if (minutes > 0) {
      if (isTr) return seconds > 0 ? `${minutes} dakika ${seconds} saniye` : `${minutes} dakika`;
      if (isZh) return seconds > 0 ? `${minutes} 分钟 ${seconds} 秒` : `${minutes} 分钟`;
      if (isPt) return seconds > 0 ? `${minutes} minutos e ${seconds} segundos` : `${minutes} minutos`;
      return seconds > 0 ? `${minutes} minutes ${seconds} seconds` : `${minutes} minutes`;
    }
    if (isTr) return `${seconds} saniye`;
    if (isZh) return `${seconds} 秒`;
    if (isPt) return `${seconds} segundos`;
    return `${seconds} seconds`;
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

  const renderQuotaBadges = (metricsList: PluginResourceMetric[]) => {
    if (!metricsList || metricsList.length === 0) return null;

    const relevant = metricsList.filter(
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
      <div className={styles.quotaBadgesContainer} data-single={relevant.length <= 1}>
        {relevant.map((m) => {
          let badgeLabel = "Kota";
          let fullName = "Model";
          let fullPeriod = "Kota";
          let fillColor = "#3186FF";

          if (m.id === "claude-quota") {
            badgeLabel = "Claude (5 Saat)";
            fullName = "Claude";
            fullPeriod = "5 Saatlik Oturum Kotası";
            fillColor = m.value > 20 ? "#D97757" : "#ef4444";
          } else if (m.id === "claude-weekly-quota") {
            badgeLabel = "Claude (Hafta)";
            fullName = "Claude";
            fullPeriod = "Haftalık Toplam Kota";
            fillColor = m.value > 20 ? "#D97757" : "#ef4444";
          } else if (m.id === "gemini-quota") {
            badgeLabel = "Gemini (5 Saat)";
            fullName = "Gemini";
            fullPeriod = "5 Saatlik Oturum Kotası";
            fillColor = m.value > 20 ? "#3186FF" : "#ef4444";
          } else if (m.id === "gemini-weekly-quota") {
            badgeLabel = "Gemini (Hafta)";
            fullName = "Gemini";
            fullPeriod = "Haftalık Toplam Kota";
            fillColor = m.value > 20 ? "#3186FF" : "#ef4444";
          } else if (m.id === "primary-quota") {
            badgeLabel = "Codex (5 Saat)";
            fullName = "OpenAI Codex";
            fullPeriod = "5 Saatlik Oturum Kotası";
            fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
          } else if (m.id === "secondary-quota") {
            badgeLabel = "Codex (Hafta)";
            fullName = "OpenAI Codex";
            fullPeriod = "Haftalık Toplam Kota";
            fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
          } else if (m.id === "copilot-status") {
            badgeLabel = "Copilot";
            fullName = "GitHub Copilot";
            fullPeriod = "Aktif Abonelik";
            fillColor = "#238636";
          } else if (m.id === "credits") {
            badgeLabel = "Grok (Hafta)";
            fullName = "xAI Grok";
            fullPeriod = "Haftalık Kredi";
            fillColor = m.value > 20 ? "#e0e0e0" : "#ef4444";
          } else if (m.id === "kimi-5h") {
            badgeLabel = "Kimi (5 Saat)";
            fullName = "Moonshot Kimi";
            fullPeriod = "5 Saatlik Oturum Kotası";
            fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
          } else if (m.id === "kimi-weekly") {
            badgeLabel = "Kimi (Hafta)";
            fullName = "Moonshot Kimi";
            fullPeriod = "Haftalık Toplam Kota";
            fillColor = m.value > 20 ? "#10a37f" : "#ef4444";
          } else if (m.id === "claude-code-5h") {
            badgeLabel = "Claude (5 Saat)";
            fullName = "Claude Code";
            fullPeriod = "5 Saatlik Oturum Kotası";
            fillColor = m.value > 20 ? "#D97757" : "#ef4444";
          } else if (m.id === "claude-code-weekly") {
            badgeLabel = "Claude (Hafta)";
            fullName = "Claude Code";
            fullPeriod = "Haftalık Toplam Kota";
            fillColor = m.value > 20 ? "#D97757" : "#ef4444";
          } else if (m.id === "claude-code-sonnet") {
            badgeLabel = "Sonnet (Hafta)";
            fullName = "Claude Code Sonnet";
            fullPeriod = "Haftalık Özel Kota";
            fillColor = m.value > 20 ? "#D97757" : "#ef4444";
          }

          const percent = Math.round(Math.max(0, Math.min(100, m.value)));
          let tooltip =
            m.id === "copilot-status"
              ? "GitHub Copilot: Aktif Abonelik (Sınırsız Kullanım)"
              : `${fullName} (${fullPeriod}): %${percent} kalan`;

          if (m.resetAtMs) {
            const date = new Date(m.resetAtMs);
            tooltip += ` · Sıfırlanma: ${date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
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
  };

  const dashboardContent = (
    <div className={styles.dashboard}>
      {/* ── 1. Hero KPI Grid (3 Balanced, Equal Metric Cards) ── */}
      <div className={styles.heroGrid3}>
        {/* Card 1: Token Usage */}
        <Link to="/calls" className={styles.heroCardKpiLink}>
          <div className={styles.heroCardKpi}>
            <div className={styles.heroCardHeader}>
              <span className={styles.heroCardTitle}>{t("home.usage_metrics_title")}</span>
              <span className={styles.statusBadgeOk}>Token Hacmi</span>
            </div>
            <div className={styles.heroValue}>
              {formatCompactInteger(netTotal)} Token
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
              <span className={styles.heroCardTitle}>Önbellek (Cache)</span>
              <span className={styles.statusBadgeOk}>
                %{Math.round((cachedTokens / Math.max(1, metrics.prompt_tokens)) * 100)} Oran
              </span>
            </div>
            <div className={styles.heroValue}>
              {formatCompactInteger(cachedTokens)} Token
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
              <span>Toplam {formatCompactInteger(metrics.prompt_tokens)} bağlamın {formatCompactInteger(cachedTokens)} token'ı önbellekten okundu</span>
            </div>
          </div>
        </Link>

        {/* Card 3: Calls & Success Rate */}
        <Link to="/calls" className={styles.heroCardKpiLink}>
          <div className={styles.heroCardKpi}>
            <div className={styles.heroCardHeader}>
              <span className={styles.heroCardTitle}>Çağrı & Sağlık</span>
              <span className={styles.statusBadgeOk}>%{successRate} Başarı</span>
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
              <span>Kesintisiz Akış</span>
            </div>
          </div>
        </Link>
      </div>

      {/* ── Optional Alert Banners: Live Cooling Down Accounts (One per line) ── */}
      {coolingOrExhaustedItems.length > 0 && (
        <div className={styles.coolingAlertContainer}>
          {coolingOrExhaustedItems.map((item) => {
            const remainingStr = formatRemainingDuration(item.retryAtMs);
            let noticeText = "";

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

            return (
              <div
                key={item.id}
                className={styles.coolingAlertBanner}
                onClick={() => navigate("/models")}
                title="Modeller sayfasına git"
              >
                <div className={styles.coolingAlertIcon}>
                  <Icon icon={clockIcon} />
                </div>
                <div className={styles.coolingAlertText}>
                  {noticeText}
                </div>
              </div>
            );
          })}
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
                ● {t("home.status_ready_count", { count: readyAccounts.length })}
              </span>
              {coolingAccounts.length > 0 && (
                <span className={styles.metaItemWarn}>
                  ● {t("home.status_cooling_count", { count: coolingAccounts.length })}
                </span>
              )}
              {disabledAccounts.length > 0 && (
                <span className={styles.metaItemDisabled}>
                  ● {t("home.status_disabled_count", { count: disabledAccounts.length })}
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
                    onClick={() => navigate("/models")}
                    style={{ cursor: "pointer" }}
                  >
                    <div className={styles.providerLeft}>
                      {logo && <img src={logo} alt="" className={styles.providerLogo} />}
                      <span className={styles.providerName}>{p.name}</span>
                      <span className={styles.providerAccountsBadge}>
                        {accounts.length} Hesap
                      </span>
                    </div>

                    <div className={styles.providerRight}>
                      {firstAccount && renderQuotaBadges(firstAccount.metrics)}
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
          title="Son işlenen model çağrısı (Canlı Trafik)"
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
