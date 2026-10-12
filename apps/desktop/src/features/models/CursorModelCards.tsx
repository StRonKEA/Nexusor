import type { IconifyIcon } from "@iconify/react/offline";
import { useMemo, useState } from "react";
import { api, type Model, type PluginDescriptor, type PluginModelDescriptor } from "../../shared/api";
import { appStore } from "../../shared/store/appStore";
import { Card } from "../../shared/ui/Card";
import { Icon } from "../../shared/ui/Icon";
import { Switch } from "../../shared/ui/Switch";
import {
  providerIcon,
  searchIcon,
} from "../../shared/ui/icons";
import { getProviderLogo } from "../../shared/utils/providerIcons";
import { TruncatedButton } from "../../shared/ui/TruncatedButton";
import { CursorModelTestResult, type CursorModelTestState } from "./CursorModelTestResult";
import styles from "./CursorSettings.module.scss";

export type CursorModelGrouping = "flat" | "provider" | "type";

export type CursorModelGroup = {
  key: string;
  label: string;
  icon: IconifyIcon;
  iconSrc?: string;
  models: Model[];
};

type CursorModelCardsProps = {
  models: Model[];
  pluginModels: PluginModelDescriptor[];
  plugins?: PluginDescriptor[];
  grouping: CursorModelGrouping;
  disabled: boolean;
  testingModelHashes: Set<string>;
  testResults: Map<string, CursorModelTestState>;
  onTest: (model: Model) => void;
  onEdit: (model: Model) => void;
  onDuplicate: (model: Model) => void;
  onDelete: (model: Model) => void;
  onTestPluginModel: (model: PluginModelDescriptor) => void;
  onPluginSettings: (model: PluginModelDescriptor) => void;
  onReorder?: (modelHashes: string[]) => void;
  onGroupSettings?: (group: CursorModelGroup) => void;
};

export function cursorModelGroups(
  models: Model[],
  grouping: Exclude<CursorModelGrouping, "flat">
): CursorModelGroup[] {
  const groups = new Map<string, CursorModelGroup>();
  for (const model of models) {
    const descriptor = grouping === "provider" ? providerGroup(model) : typeGroup(model);
    const group = groups.get(descriptor.key);
    if (group) {
      group.models.push(model);
    } else {
      groups.set(descriptor.key, { ...descriptor, models: [model] });
    }
  }
  return [...groups.values()];
}

export function CursorModelCards(props: CursorModelCardsProps) {
  const [filterTab, setFilterTab] = useState<"all" | "native" | "custom">("all");
  const [searchQuery, setSearchQuery] = useState("");

  // Helper to map plugin ID to plugin name and accounts
  const pluginInfoMap = useMemo(() => {
    const map = new Map<
      string,
      {
        name: string;
        accounts: string[];
      }
    >();
    if (!props.plugins) return map;
    for (const plugin of props.plugins) {
      const accounts: string[] = [];
      for (const res of plugin.resources) {
        for (const item of res.resources) {
          if (item.displayName) {
            accounts.push(item.displayName);
          }
        }
      }
      map.set(plugin.id, {
        name: plugin.name,
        accounts,
      });
    }
    return map;
  }, [props.plugins]);

  // Built-in Native Plugin Models (deduplicated by modelId)
  const uniqueNativeModels = useMemo(() => {
    const map = new Map<string, PluginModelDescriptor>();
    for (const m of props.pluginModels) {
      if (!map.has(m.id)) {
        map.set(m.id, m);
      }
    }
    return Array.from(map.values());
  }, [props.pluginModels]);

  const filteredNativeModels = useMemo(() => {
    if (filterTab === "custom") return [];
    const q = searchQuery.trim().toLowerCase();
    return uniqueNativeModels.filter((m) => {
      if (!q) return true;
      return (
        m.displayName.toLowerCase().includes(q) ||
        m.modelId.toLowerCase().includes(q) ||
        m.pluginId.toLowerCase().includes(q)
      );
    });
  }, [uniqueNativeModels, filterTab, searchQuery]);

  const filteredCustomModels = useMemo(() => {
    if (filterTab === "native") return [];
    const q = searchQuery.trim().toLowerCase();
    return props.models.filter((m) => {
      if (!q) return true;
      const group = m.group_name?.toLowerCase() || "";
      return (
        m.display_name.toLowerCase().includes(q) ||
        m.model_id.toLowerCase().includes(q) ||
        group.includes(q)
      );
    });
  }, [props.models, filterTab, searchQuery]);

  const totalCount = uniqueNativeModels.length + props.models.length;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "14px" }}>
      {/* ── Toolbar: Category Filters & Search ── */}
      <div className={styles.catalogToolbar}>
        <div className={styles.catalogFilterTabs}>
          <button
            type="button"
            className={`${styles.catalogTabBtn} ${filterTab === "all" ? styles.catalogTabActive : ""}`}
            onClick={() => setFilterTab("all")}
          >
            {t("calls.all_models")} ({totalCount})
          </button>
          <button
            type="button"
            className={`${styles.catalogTabBtn} ${filterTab === "native" ? styles.catalogTabActive : ""}`}
            onClick={() => setFilterTab("native")}
          >
            {t("models.native_providers_title")} ({uniqueNativeModels.length})
          </button>
          <button
            type="button"
            className={`${styles.catalogTabBtn} ${filterTab === "custom" ? styles.catalogTabActive : ""}`}
            onClick={() => setFilterTab("custom")}
          >
            {t("models.custom_providers_title")} ({props.models.length})
          </button>
        </div>

        <div className={styles.catalogSearchWrap}>
          <Icon icon={searchIcon} className={styles.catalogSearchIcon} />
          <input
            type="text"
            className={styles.catalogSearchInput}
            placeholder={t("calls.search_placeholder")}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
        </div>
      </div>

      {/* ── Native Models Section ── */}
      {filteredNativeModels.length > 0 && (
        <div className={styles.sectionBlock}>
          {filterTab === "all" && (
            <div className={styles.sectionHeaderRow}>
              <h3 className={styles.sectionTitle}>{t("models.native_providers_title")}</h3>
            </div>
          )}
          <div className={styles.catalogList}>
            {filteredNativeModels.map((model) => {
              const logo = getProviderLogo(model.pluginId);
              const info = pluginInfoMap.get(model.pluginId);
              const providerName = info?.name || model.pluginName || model.pluginId;
              const accountCount = info?.accounts.length ?? 0;
              const accountText =
                accountCount === 0
                  ? t("providers.not_connected")
                  : accountCount === 1
                  ? info!.accounts[0]
                  : t("combos.account_pool_format", { count: accountCount, accounts: info!.accounts.slice(0, 2).join(", ") });

              return (
                <div key={model.id} className={styles.modelCatalogRow}>
                  <div className={styles.modelRowLeft}>
                    {logo ? (
                      <img src={logo} alt="" className={styles.modelProviderLogo} />
                    ) : (
                      <div className={styles.modelProviderLogoPlaceholder}>
                        <Icon icon={providerIcon} />
                      </div>
                    )}
                    <div className={styles.modelRowInfo}>
                      <div style={{ display: "flex", alignItems: "center", gap: "8px", flexWrap: "wrap" }}>
                        <span className={styles.modelRowNameText}>{model.displayName}</span>
                        <span className={styles.modelProviderBadge}>
                          {providerName}
                        </span>
                        {accountCount > 0 ? (
                          <span className={styles.modelAccountBadge} title={info?.accounts.join("\n")}>
                            {accountText}
                          </span>
                        ) : (
                          <span className={styles.modelAccountBadgeUnlinked}>
                            {t("providers.not_connected")}
                          </span>
                        )}
                      </div>
                      <span className={styles.modelRowModelId}>{model.modelId}</span>
                    </div>
                  </div>

                  <div className={styles.modelRowRight}>
                    <CursorModelTestResult compact state={props.testResults.get(model.id)} testing={props.testingModelHashes.has(model.id)} />
                    <Switch
                      checked={model.enabled}
                      disabled={props.disabled}
                      label={t("plugins.enable", { model: model.displayName })}
                      onChange={async (enabled) => {
                        await api.setPluginModelEnabled(model.pluginId, model.providerId, model.modelId, enabled);
                        await appStore.refreshPlugins();
                      }}
                    />
                    <div className={styles.modelCardActions}>
                      <TruncatedButton
                        size="small"
                        disabled={props.disabled && !props.testingModelHashes.has(model.id)}
                        label={props.testingModelHashes.has(model.id) ? t("models.cancel_test") : t("models.test")}
                        onClick={() => props.onTestPluginModel(model)}
                      />
                      <TruncatedButton
                        size="small"
                        disabled={props.disabled}
                        label={t("models.settings")}
                        onClick={() => props.onPluginSettings(model)}
                      />
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* ── Custom Models Section ── */}
      {filteredCustomModels.length > 0 && (
        <div className={styles.sectionBlock}>
          {filterTab === "all" && (
            <div className={styles.sectionHeaderRow}>
              <h3 className={styles.sectionTitle}>{t("models.custom_providers_title")}</h3>
            </div>
          )}
          <div className={styles.catalogList}>
            {filteredCustomModels.map((model) => {
              const logo = getProviderLogo(model.group_name) || getProviderLogo(model.base_url);
              return (
                <div key={model.model_hash} className={styles.modelCatalogRow}>
                  <div className={styles.modelRowLeft}>
                    {logo ? (
                      <img src={logo} alt="" className={styles.modelProviderLogo} />
                    ) : (
                      <div className={styles.modelProviderLogoPlaceholder}>
                        <Icon icon={providerIcon} />
                      </div>
                    )}
                    <div className={styles.modelRowInfo}>
                      <div style={{ display: "flex", alignItems: "center", gap: "8px", flexWrap: "wrap" }}>
                        <span className={styles.modelRowNameText}>{model.display_name}</span>
                        {model.group_name && (
                          <span className={styles.modelProviderBadge}>
                            {model.group_name}
                          </span>
                        )}
                        <span className={styles.modelAccountBadge}>
                          {model.type.toUpperCase()}
                        </span>
                      </div>
                      <span className={styles.modelRowModelId}>{model.model_id}</span>
                    </div>
                  </div>

                  <div className={styles.modelRowRight}>
                    <CursorModelTestResult compact state={props.testResults.get(model.model_hash)} testing={props.testingModelHashes.has(model.model_hash)} />
                    <div className={styles.modelCardActions}>
                      <TruncatedButton
                        size="small"
                        disabled={props.disabled && !props.testingModelHashes.has(model.model_hash)}
                        label={props.testingModelHashes.has(model.model_hash) ? t("models.cancel_test") : t("models.test")}
                        onClick={() => props.onTest(model)}
                      />
                      <TruncatedButton
                        size="small"
                        disabled={props.disabled}
                        label={t("common.edit")}
                        onClick={() => props.onEdit(model)}
                      />
                      <TruncatedButton
                        size="small"
                        disabled={props.disabled}
                        label={t("models.duplicate")}
                        onClick={() => props.onDuplicate(model)}
                      />
                      <TruncatedButton
                        size="small"
                        className={styles.deleteButton}
                        disabled={props.disabled}
                        label={t("common.delete")}
                        onClick={() => props.onDelete(model)}
                      />
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {filteredNativeModels.length === 0 && filteredCustomModels.length === 0 && (
        <Card style={{ padding: "28px", textAlign: "center", color: "var(--vscode-descriptionForeground)" }}>
          {t("common.no_data")}
        </Card>
      )}
    </div>
  );
}

function providerGroup(model: Model) {
  const groupName = model.group_name?.trim();
  const domain = providerDomain(model.base_url);
  const key = groupName || domain;
  const label = groupName || domain;
  const logo = getProviderLogo(groupName) || getProviderLogo(model.base_url);
  return { key, label, icon: providerIcon, iconSrc: logo ?? undefined };
}

function providerDomain(baseUrl: string) {
  const value = baseUrl.trim();
  try {
    return new URL(value).hostname.toLowerCase() || value;
  } catch {
    return value;
  }
}

function typeGroup(model: Model) {
  return { key: model.type, label: model.type, icon: providerIcon };
}
