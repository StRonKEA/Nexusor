import { useSyncExternalStore } from "react";
import { api, type CursorHarnessStatus, type LlmCall, type Model, type ModelInput, type Overview, type PluginDescriptor, type PluginRuntimeStatus, type PortSettings, type TokenPricingSettings } from "../api";
import { THEME_ID } from "../theme/theme";
import { ensureNotificationPermission, sendDesktopNotification } from "../native/notifications";

const knownAccountStates = new Map<string, string>();
const knownAccountQuotas = new Map<string, number>();

function extractLowestQuotaPercent(item: { metrics: Array<{ unit: string; value: number }> }): number | null {
  const pctMetrics = item.metrics.filter((m) => m.unit === "percent");
  if (pctMetrics.length === 0) return null;
  return Math.min(...pctMetrics.map((m) => Math.round(m.value)));
}

export const DEFAULT_TOKEN_PRICING: TokenPricingSettings = {
  input_per_million: 5.0,
  output_per_million: 25.0,
  cache_read_per_million: 0.5,
  cache_write_per_million: 6.25,
};

export type AppSnapshot = {
  models: Model[];
  calls: LlmCall[];
  overview: Overview;
  detailed: boolean;
  ports: PortSettings;
  pricing: TokenPricingSettings;
  busy: boolean;
  error: string | null;
  theme: typeof THEME_ID;
  cursorHarness: CursorHarnessStatus | null;
  cursorBusy: boolean;
  pluginRuntime: PluginRuntimeStatus | null;
  plugins: PluginDescriptor[];
};

let snapshot: AppSnapshot = {
  models: [],
  calls: [],
  overview: {
    metrics: {
      llm_calls: 0,
      successful_calls: 0,
      failed_calls: 0,
      token_usage: 0,
      prompt_tokens: 0,
      input_tokens: 0,
      cache_read_tokens: 0,
      cache_write_tokens: 0,
      output_tokens: 0,
    },
    token_usage_granularity: "day",
    token_usage_series: [],
  },
  detailed: false,
  ports: { proxy_port: 0, service_port: 0 },
  pricing: DEFAULT_TOKEN_PRICING,
  busy: false,
  error: null,
  theme: THEME_ID,
  cursorHarness: null,
  cursorBusy: false,
  pluginRuntime: null,
  plugins: [],
};

const listeners = new Set<() => void>();

function update(patch: Partial<AppSnapshot>) {
  snapshot = { ...snapshot, ...patch };
  listeners.forEach((listener) => listener());
}

async function perform(task: () => Promise<void>) {
  update({ error: null });
  try {
    await task();
  } catch (cause) {
    update({ error: cause instanceof Error ? cause.message : String(cause) });
  }
}

export const appStore = {
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
  getSnapshot: () => snapshot,

  async refresh() {
    update({ busy: true, error: null });
    try {
      const [
        modelsRes,
        callsRes,
        overviewRes,
        settingsRes,
        portsRes,
        pricingRes,
        cursorHarnessRes,
        pluginRuntimeRes,
        pluginsRes,
      ] = await Promise.allSettled([
        api.models(),
        api.calls(),
        api.overview(),
        api.observability(),
        api.ports(),
        api.pricingSettings(),
        api.cursorHarness(),
        api.pluginRuntime(),
        api.plugins(),
      ]);

      const patch: Partial<AppSnapshot> = {};
      const errors: string[] = [];

      if (modelsRes.status === "fulfilled") patch.models = modelsRes.value;
      else errors.push(`models: ${modelsRes.reason}`);

      if (callsRes.status === "fulfilled") patch.calls = callsRes.value;
      else errors.push(`calls: ${callsRes.reason}`);

      if (overviewRes.status === "fulfilled") patch.overview = overviewRes.value;
      else errors.push(`overview: ${overviewRes.reason}`);

      if (settingsRes.status === "fulfilled") patch.detailed = settingsRes.value.detailed;
      else errors.push(`observability: ${settingsRes.reason}`);

      if (portsRes.status === "fulfilled") patch.ports = portsRes.value;
      else errors.push(`ports: ${portsRes.reason}`);

      if (pricingRes.status === "fulfilled") patch.pricing = pricingRes.value;
      else errors.push(`pricing: ${pricingRes.reason}`);

      if (cursorHarnessRes.status === "fulfilled") patch.cursorHarness = cursorHarnessRes.value;
      else errors.push(`cursorHarness: ${cursorHarnessRes.reason}`);

      if (pluginRuntimeRes.status === "fulfilled") patch.pluginRuntime = pluginRuntimeRes.value;
      else errors.push(`pluginRuntime: ${pluginRuntimeRes.reason}`);

      if (pluginsRes.status === "fulfilled") {
        patch.plugins = pluginsRes.value;

        // Check for state transitions (ready <-> cooling) and send desktop notifications
        ensureNotificationPermission();
        const isFirstLoad = knownAccountStates.size === 0;

        for (const p of pluginsRes.value) {
          for (const rg of p.resources) {
            for (const item of rg.resources) {
              const currentStatus = item.state.status;
              const prevStatus = knownAccountStates.get(item.id);

              if (!isFirstLoad && prevStatus && prevStatus !== currentStatus) {
                if (prevStatus === "ready" && currentStatus === "cooling") {
                  sendDesktopNotification(
                    t("notifications.account_cooling_title", { provider: p.name }),
                    t("notifications.account_cooling_body", { account: item.displayName || p.name })
                  );
                } else if (prevStatus === "cooling" && currentStatus === "ready") {
                  sendDesktopNotification(
                    t("notifications.account_ready_title", { provider: p.name }),
                    t("notifications.account_ready_body", { account: item.displayName || p.name })
                  );
                }
              }

              knownAccountStates.set(item.id, currentStatus);

              // Proactive quota threshold warning
              const thresholdVal = Number(
                (typeof localStorage !== "undefined" && localStorage.getItem("nexusor_quota_alert_threshold")) || "15"
              );
              if (thresholdVal > 0) {
                const currentRemaining = extractLowestQuotaPercent(item);
                const prevRemaining = knownAccountQuotas.get(item.id);

                if (
                  !isFirstLoad &&
                  currentRemaining !== null &&
                  prevRemaining !== undefined &&
                  prevRemaining > thresholdVal &&
                  currentRemaining <= thresholdVal &&
                  currentRemaining > 0
                ) {
                  sendDesktopNotification(
                    t("notifications.quota_warning_title", { provider: p.name }),
                    t("notifications.quota_warning_body", {
                      account: item.displayName || p.name,
                      percent: currentRemaining,
                      threshold: thresholdVal,
                    })
                  );
                }

                if (currentRemaining !== null) {
                  knownAccountQuotas.set(item.id, currentRemaining);
                }
              }
            }
          }
        }
      }
      else errors.push(`plugins: ${pluginsRes.reason}`);

      if (Object.keys(patch).length > 0) {
        update(patch);
      }
      if (errors.length > 0) {
        update({ error: errors.join(", ") });
      }
    } catch (cause) {
      update({ error: cause instanceof Error ? cause.message : String(cause) });
    } finally {
      update({ busy: false });
    }
  },

  async deleteModel(modelHash: string) {
    await perform(async () => {
      await api.deleteModel(modelHash);
      await appStore.refresh();
    });
  },

  async initializeCursorCa() {
    update({ cursorBusy: true, error: null });
    try {
      const status = await api.initializeCursorCa();
      update({ cursorHarness: status });
      return status;
    } catch (cause) {
      update({ error: cause instanceof Error ? cause.message : String(cause) });
      return null;
    } finally { update({ cursorBusy: false }); }
  },
  async initializePluginRuntime() {
    update({ error: null });
    try {
      const pluginRuntime = await api.initializePluginRuntime();
      update({ pluginRuntime });
      return pluginRuntime;
    } catch (cause) {
      update({ error: cause instanceof Error ? cause.message : String(cause) });
      return null;
    }
  },
  async refreshPluginRuntime() {
    try {
      const wasReady = snapshot.pluginRuntime?.state === "ready";
      const pluginRuntime = await api.pluginRuntime();
      update({ pluginRuntime });
      if (!wasReady && pluginRuntime.state === "ready") {
        const plugins = await api.plugins();
        update({ plugins });
      }
      return pluginRuntime;
    } catch (cause) {
      update({ error: cause instanceof Error ? cause.message : String(cause) });
      return null;
    }
  },
  async cancelPluginRuntimeInitialization() {
    try {
      const pluginRuntime = await api.cancelPluginRuntimeInitialization();
      update({ pluginRuntime });
      return pluginRuntime;
    } catch (cause) {
      update({ error: cause instanceof Error ? cause.message : String(cause) });
      return null;
    }
  },
  async refreshPlugins() {
    try {
      update({ plugins: await api.plugins() });
    } catch (cause) {
      update({ error: cause instanceof Error ? cause.message : String(cause) });
    }
  },
  async removePluginConfiguration(pluginId: string) {
    await perform(async () => {
      await api.removePluginConfiguration(pluginId);
      update({ plugins: await api.plugins() });
    });
  },
  async setCursorEnabled(enabled: boolean) {
    update({ cursorBusy: true, error: null });
    try { update({ cursorHarness: await api.setCursorEnabled(enabled) }); }
    catch (cause) { update({ error: cause instanceof Error ? cause.message : String(cause) }); }
    finally { update({ cursorBusy: false }); }
  },
  async createModels(models: ModelInput[]) {
    update({ cursorBusy: true, error: null });
    try {
      const created = await api.createModels(models);
      await appStore.refresh();
      return created;
    } catch (cause) {
      update({ error: cause instanceof Error ? cause.message : String(cause) });
      return null;
    } finally { update({ cursorBusy: false }); }
  },
  async importV0049Models() {
    update({ cursorBusy: true, error: null });
    try {
      const result = await api.importV0049Models();
      await appStore.refresh();
      return result;
    } catch (cause) {
      update({ error: cause instanceof Error ? cause.message : String(cause) });
      return null;
    } finally { update({ cursorBusy: false }); }
  },
  async updateCursorModel(hash: string, model: ModelInput) {
    update({ cursorBusy: true, error: null });
    try {
      const updated = await api.updateModel(hash, model);
      await appStore.refresh();
      return updated;
    } catch (cause) {
      update({ error: cause instanceof Error ? cause.message : String(cause) });
      return null;
    } finally { update({ cursorBusy: false }); }
  },
  async reorderCursorModels(modelHashes: string[]) {
    const previous = snapshot.models;
    const byHash = new Map(previous.map((model) => [model.model_hash, model]));
    if (modelHashes.length !== previous.length || new Set(modelHashes).size !== previous.length) {
      update({ error: t("common.the_model_configuration_changed_") });
      return false;
    }
    const reordered: Model[] = [];
    for (const [index, hash] of modelHashes.entries()) {
      const model = byHash.get(hash);
      if (!model) {
        update({ error: t("common.the_model_configuration_changed_") });
        return false;
      }
      reordered.push({ ...model, sort_order: index + 1 });
    }
    update({ models: reordered, cursorBusy: true, error: null });
    try {
      update({ models: await api.reorderModels(modelHashes) });
      return true;
    } catch (cause) {
      update({
        models: previous,
        error: cause instanceof Error ? cause.message : String(cause),
      });
      return false;
    } finally {
      update({ cursorBusy: false });
    }
  },

  async refreshCalls() {
    try {
      update({ calls: await api.calls() });
    } catch (cause) {
      update({ error: cause instanceof Error ? cause.message : String(cause) });
    }
  },

  async openCallDetails(callId: string) {
    await perform(() => api.openCallDetails(callId));
  },
  async updateDetailed(detailed: boolean) {
    await perform(async () => update(await api.setObservability(detailed)));
  },
  async updatePorts(ports: PortSettings) {
    try {
      update({ error: null });
      update({ ports: await api.setPorts(ports) });
      return true;
    } catch (cause) {
      update({ error: cause instanceof Error ? cause.message : String(cause) });
      return false;
    }
  },
  async updatePricingSettings(pricing: TokenPricingSettings) {
    try {
      update({ error: null });
      update({ pricing: await api.setPricingSettings(pricing) });
      return true;
    } catch (cause) {
      update({ error: cause instanceof Error ? cause.message : String(cause) });
      return false;
    }
  },
};

export function useAppStore() {
  return useSyncExternalStore(appStore.subscribe, appStore.getSnapshot);
}
