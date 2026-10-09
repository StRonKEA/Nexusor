import type { EChartsCoreOption } from "echarts/core";
import { useMemo, useState } from "react";
import type { TokenUsageGranularity } from "../../../shared/api";
import { formatCompactInteger } from "../../../shared/utils/numberFormat";
import { EChart } from "./EChart";
import styles from "./DailyTokenUsageChart.module.scss";

export type DailyTokenUsage = {
  bucketStartMs: number;
  inputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  outputTokens: number;
};

type TooltipItem = {
  dataIndex: number;
};

function colorMark(color: string): string {
  return `<span style="display:inline-block;width:7px;height:7px;margin-right:6px;border-radius:50%;background-color:${color};vertical-align:middle"></span>`;
}

function formatDay(value: Date) {
  return `${value.getUTCMonth() + 1}/${value.getUTCDate()}`;
}

function pad(value: number) {
  return String(value).padStart(2, "0");
}

function formatAxisLabel(bucketStartMs: number, granularity: TokenUsageGranularity) {
  const value = new Date(bucketStartMs);
  if (granularity === "minute") return `${pad(value.getHours())}:${pad(value.getMinutes())}`;
  if (granularity === "hour") return `${pad(value.getHours())}:00`;
  const day = value.getUTCDay();
  if (day === 6) return t("home.saturday");
  if (day === 0) return t("home.sunday");
  return formatDay(value);
}

function formatTooltipTime(bucketStartMs: number, granularity: TokenUsageGranularity) {
  const value = new Date(bucketStartMs);
  if (granularity === "day") {
    return formatDay(value);
  }
  const time = `${pad(value.getHours())}:${pad(value.getMinutes())}`;
  return `${value.getMonth() + 1}/${value.getDate()} ${time}`;
}

function totalTokens(day: DailyTokenUsage) {
  return day.inputTokens + day.outputTokens;
}

export function DailyTokenUsageChart({
  data,
  granularity,
}: {
  data: DailyTokenUsage[];
  granularity: TokenUsageGranularity;
}) {
  const [hovered, setHovered] = useState(false);

  // 1. KPI Calculations
  const totalTokensWeek = useMemo(() => {
    return data.reduce((sum, day) => sum + totalTokens(day), 0);
  }, [data]);

  const dailyAverageTokens = useMemo(() => {
    if (data.length === 0) return 0;
    return Math.round(totalTokensWeek / data.length);
  }, [totalTokensWeek, data]);

  const cacheEfficiency = useMemo(() => {
    let cacheRead = 0;
    let input = 0;
    for (const d of data) {
      cacheRead += d.cacheReadTokens;
      input += d.inputTokens;
    }
    const total = cacheRead + input;
    if (total === 0) return 0;
    return Math.round((cacheRead / total) * 100);
  }, [data]);

  const maximumTotal = data.reduce((maximum, day) => Math.max(maximum, totalTokens(day)), 0);
  const axisMaximum = Math.max(100, Math.ceil((maximumTotal * 1.15) / 100) * 100);

  const option = useMemo<EChartsCoreOption>(() => ({
    animationDuration: 600,
    animationEasing: "cubicOut",
    grid: { top: 20, right: 16, bottom: 24, left: 42 },
    tooltip: {
      trigger: "axis",
      confine: true,
      backgroundColor: "rgba(17, 18, 22, 0.94)",
      borderColor: "rgba(255, 255, 255, 0.12)",
      textStyle: { color: "#f7f8f8", fontSize: 12 },
      extraCssText:
        "border-radius: 8px; box-shadow: 0 16px 36px rgba(0, 0, 0, 0.7); backdrop-filter: blur(10px); padding: 10px 14px; line-height: 1.6;",
      axisPointer: {
        type: "line",
        lineStyle: { color: "rgba(94, 106, 210, 0.5)", width: 1.5, type: "dashed" },
      },
      formatter: (params: unknown) => {
        const first = (params as TooltipItem[])[0];
        const day = data[first.dataIndex];
        const total = totalTokens(day);
        const dateStr = formatTooltipTime(day.bucketStartMs, granularity);
        return [
          `<div style="font-weight:600;font-size:12.5px;color:#f7f8f8;margin-bottom:4px;">${dateStr}</div>`,
          `<div style="display:flex;justify-content:space-between;gap:16px;margin-bottom:6px;font-size:12px;">`,
          `<span style="color:#8a8f98;">${t("home.total_requests")}:</span>`,
          `<strong style="color:#7075f5;font-family:monospace;">${formatCompactInteger(total)} Token</strong>`,
          `</div>`,
          `<div style="font-size:11px;color:#b0b4ba;display:flex;flex-direction:column;gap:3px;border-top:1px solid rgba(255,255,255,0.06);padding-top:5px;">`,
          `<div>${colorMark("#5e6ad2")} ${t("home.input_non_cached")}: <b style="color:#fff;font-family:monospace;">${formatCompactInteger(day.inputTokens)}</b></div>`,
          `<div>${colorMark("#f2994a")} ${t("home.model_output")}: <b style="color:#fff;font-family:monospace;">${formatCompactInteger(day.outputTokens)}</b></div>`,
          day.cacheReadTokens > 0 ? `<div>${colorMark("#27c376")} ${t("home.cached_input")}: <b style="color:#fff;font-family:monospace;">${formatCompactInteger(day.cacheReadTokens)}</b></div>` : "",
          `</div>`,
        ].filter(Boolean).join("");
      },
    },
    xAxis: {
      type: "category",
      boundaryGap: false,
      data: data.map(({ bucketStartMs }) => bucketStartMs),
      axisTick: { show: false },
      axisLine: { lineStyle: { color: "rgba(255, 255, 255, 0.08)" } },
      axisLabel: {
        interval: "auto",
        hideOverlap: true,
        formatter: (_value: string, index: number) =>
          formatAxisLabel(data[index].bucketStartMs, granularity),
        color: "#8a8f98",
        fontSize: 11,
        margin: 12,
      },
    },
    yAxis: {
      type: "value",
      show: true,
      min: 0,
      max: axisMaximum,
      splitLine: {
        lineStyle: { color: "rgba(255, 255, 255, 0.04)", type: "dashed" },
      },
      axisLabel: {
        color: "#6b7280",
        fontSize: 10,
        formatter: (val: number) => formatCompactInteger(val),
      },
    },
    series: [
      {
        name: "Tokens",
        type: "line",
        smooth: 0.35,
        symbol: "circle",
        showSymbol: false,
        symbolSize: 6,
        data: data.map(totalTokens),
        lineStyle: {
          color: "#5e6ad2",
          width: 2.5,
          shadowColor: "rgba(94, 106, 210, 0.45)",
          shadowBlur: 10,
        },
        itemStyle: {
          color: "#5e6ad2",
          borderColor: "#ffffff",
          borderWidth: 1.5,
        },
        areaStyle: {
          color: {
            type: "linear",
            x: 0,
            y: 0,
            x2: 0,
            y2: 1,
            colorStops: [
              { offset: 0, color: "rgba(94, 106, 210, 0.35)" },
              { offset: 0.65, color: "rgba(94, 106, 210, 0.08)" },
              { offset: 1, color: "rgba(94, 106, 210, 0.0)" },
            ],
          },
        },
        markLine:
          dailyAverageTokens > 0
            ? {
                silent: true,
                symbol: "none",
                lineStyle: {
                  color: "rgba(242, 153, 74, 0.7)",
                  type: "dashed",
                  width: 1.5,
                },
                label: {
                  show: hovered,
                  position: "insideStartTop",
                  formatter: `${t("home.average")}: ${formatCompactInteger(dailyAverageTokens)}`,
                  color: "#f2994a",
                  fontSize: 10,
                },
                data: [{ yAxis: dailyAverageTokens }],
              }
            : undefined,
      },
    ],
  }), [dailyAverageTokens, axisMaximum, data, granularity, hovered]);

  return (
    <div className={styles.chartContainer}>
      <div className={styles.kpiStrip}>
        <div className={styles.kpiPill}>
          <span className={styles.kpiLabel}>{t("home.seven_days_total")}:</span>
          <span className={styles.kpiAccent}>{formatCompactInteger(totalTokensWeek)} Token</span>
        </div>
        <div className={styles.kpiPill}>
          <span className={styles.kpiLabel}>{t("home.daily_average")}:</span>
          <span className={styles.kpiValue}>{formatCompactInteger(dailyAverageTokens)}</span>
        </div>
        <div className={styles.kpiPill}>
          <span className={styles.kpiLabel}>{t("home.cache_efficiency")}:</span>
          <span className={styles.kpiAccent}>%{cacheEfficiency}</span>
        </div>
      </div>

      <EChart
        option={option}
        className={styles.chart}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
      />
    </div>
  );
}
