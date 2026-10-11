import type { IconifyIcon } from "@iconify/react/offline";
import { useState, type ReactNode } from "react";
import type { Model, PluginDescriptor, PluginModelDescriptor, PluginResourceMetric } from "../../shared/api";
import { Card } from "../../shared/ui/Card";
import { Icon } from "../../shared/ui/Icon";
import {
  chevronDownIcon,
  chevronRightIcon,
  claudeIcon,
  openAiIcon,
  providerIcon,
} from "../../shared/ui/icons";
import { getProviderLogo } from "../../shared/utils/providerIcons";
import { TruncatedButton } from "../../shared/ui/TruncatedButton";
import { QuotaBadges } from "../../shared/ui/QuotaBadges";
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

type AccountGroup = {
  key: string;
  pluginName: string;
  accountEmail: string | null;
  iconSrc?: string;
  metrics: PluginResourceMetric[];
  models: PluginModelDescriptor[];
};

function pluginAccountGroups(
  models: PluginModelDescriptor[],
  plugins: PluginDescriptor[] = []
): AccountGroup[] {
  const result: AccountGroup[] = [];

  for (const plugin of plugins) {
    const pluginModList = models.filter((m) => m.pluginId === plugin.id);
    if (pluginModList.length === 0) continue;

    const accounts = plugin.resources.flatMap((r) => r.resources);
    const iconSrc = getProviderLogo(plugin.id) || undefined;

    if (accounts.length > 0) {
      // Her hesabı ayrı bir kart olarak aç
      for (const acc of accounts) {
        result.push({
          key: `${plugin.id}:${acc.id}`,
          pluginName: plugin.name,
          accountEmail: acc.displayName?.trim() || acc.id,
          iconSrc,
          metrics: acc.metrics || [],
          models: pluginModList,
        });
      }
    } else {
      // Henüz hesap bağlanmamışsa veya tekilse
      result.push({
        key: plugin.id,
        pluginName: plugin.name,
        accountEmail: null,
        iconSrc,
        metrics: [],
        models: pluginModList,
      });
    }
  }

  return result;
}

export function CursorModelCards(props: CursorModelCardsProps) {
  const [filterMode, setFilterMode] = useState<"active" | "all">("active");
  const customGroups = cursorModelGroups(props.models, "provider");
  const allAccountGroups = pluginAccountGroups(props.pluginModels, props.plugins || []);

  // Filter groups: in "active" mode, only show groups with ready/connected accounts
  const accountGroups = filterMode === "active"
    ? allAccountGroups.filter((g) => g.accountEmail !== null)
    : allAccountGroups;

  const nativeSection = (
    <div className={styles.sectionBlock}>
      <div className={styles.sectionHeaderRow}>
        <div>
          <h3 className={styles.sectionTitle}>{t("models.native_providers_title")}</h3>
          <p className={styles.sectionSubtitle}>{t("models.native_providers_desc")}</p>
        </div>
        <div style={{ display: "flex", gap: "6px", alignItems: "center" }}>
          <button
            type="button"
            onClick={() => setFilterMode("active")}
            style={{
              padding: "4px 10px",
              borderRadius: "6px",
              fontSize: "0.82rem",
              fontWeight: filterMode === "active" ? 600 : 400,
              background: filterMode === "active" ? "rgba(49, 134, 255, 0.2)" : "rgba(255, 255, 255, 0.05)",
              color: filterMode === "active" ? "#3186FF" : "var(--vscode-descriptionForeground)",
              border: filterMode === "active" ? "1px solid rgba(49, 134, 255, 0.4)" : "1px solid transparent",
              cursor: "pointer",
            }}
          >
            {t("models.filter_active_providers", { count: allAccountGroups.filter((g) => g.accountEmail !== null).length })}
          </button>
          <button
            type="button"
            onClick={() => setFilterMode("all")}
            style={{
              padding: "4px 10px",
              borderRadius: "6px",
              fontSize: "0.82rem",
              fontWeight: filterMode === "all" ? 600 : 400,
              background: filterMode === "all" ? "rgba(49, 134, 255, 0.2)" : "rgba(255, 255, 255, 0.05)",
              color: filterMode === "all" ? "#3186FF" : "var(--vscode-descriptionForeground)",
              border: filterMode === "all" ? "1px solid rgba(49, 134, 255, 0.4)" : "1px solid transparent",
              cursor: "pointer",
            }}
          >
            {t("models.filter_all_providers", { count: allAccountGroups.length })}
          </button>
        </div>
      </div>
      <div className={styles.modelGroups}>
        {accountGroups.map((group) => {
          const headerTitle = group.accountEmail
            ? `${group.pluginName} (${group.accountEmail})`
            : group.pluginName;

          return (
            <CollapsibleGroup
              key={group.key}
              label={headerTitle}
              iconSrc={group.iconSrc}
              defaultOpen={false}
              quotaBadges={<QuotaBadges metrics={group.metrics} singleColumnThreshold={2} />}
            >
              {group.models.map((model) => (
                <PluginModelRow
                  key={`${group.key}:${model.id}`}
                  model={model}
                  disabled={props.disabled}
                  testing={props.testingModelHashes.has(model.id)}
                  result={props.testResults.get(model.id)}
                  onTest={() => props.onTestPluginModel(model)}
                  onSettings={() => props.onPluginSettings(model)}
                />
              ))}
            </CollapsibleGroup>
          );
        })}
      </div>
    </div>
  );

  const customSection = (
    <div className={styles.sectionBlock} style={{ marginTop: "24px" }}>
      <div className={styles.sectionHeaderRow}>
        <div>
          <h3 className={styles.sectionTitle}>{t("models.custom_providers_title")}</h3>
          <p className={styles.sectionSubtitle}>{t("models.custom_providers_desc")}</p>
        </div>
      </div>
      <div className={styles.modelGroups}>
        {customGroups.length === 0 ? (
          <Card style={{ padding: "16px", color: "var(--vscode-descriptionForeground)" }}>
            {t("models.no_custom_models_yet")}
          </Card>
        ) : (
          customGroups.map((group) => (
            <CollapsibleGroup
              key={group.key}
              label={group.label}
              icon={group.icon}
              iconSrc={group.iconSrc}
              defaultOpen={false}
            >
              {group.models.map((model) => (
                <ModelListRow
                  key={model.model_hash}
                  model={model}
                  disabled={props.disabled}
                  testing={props.testingModelHashes.has(model.model_hash)}
                  result={props.testResults.get(model.model_hash)}
                  onTest={() => props.onTest(model)}
                  onEdit={() => props.onEdit(model)}
                  onDuplicate={() => props.onDuplicate(model)}
                  onDelete={() => props.onDelete(model)}
                />
              ))}
            </CollapsibleGroup>
          ))
        )}
      </div>
    </div>
  );

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "10px" }}>
      {nativeSection}
      {customSection}
    </div>
  );
}

function CollapsibleGroup({
  label,
  icon,
  iconSrc,
  defaultOpen = false,
  quotaBadges,
  children,
}: {
  label: string;
  icon?: IconifyIcon;
  iconSrc?: string;
  defaultOpen?: boolean;
  quotaBadges?: ReactNode;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <Card className={styles.groupCard}>
      <div className={styles.groupHeader}>
        <button
          type="button"
          className={styles.groupToggle}
          aria-expanded={open}
          onClick={() => setOpen((current) => !current)}
        >
          {icon && <Icon icon={icon} size="1.1em" />}
          {iconSrc && <Icon src={iconSrc} size="1.1em" />}
          <span className={styles.groupLabel}>{label}</span>
        </button>

        {quotaBadges}

        <button
          type="button"
          className={styles.groupChevron}
          tabIndex={-1}
          aria-hidden="true"
          onClick={() => setOpen((current) => !current)}
        >
          <Icon icon={open ? chevronDownIcon : chevronRightIcon} size="1em" />
        </button>
      </div>
      {open && <div className={styles.modelList}>{children}</div>}
    </Card>
  );
}

function ModelListRow({
  model,
  disabled,
  testing,
  result,
  onTest,
  onEdit,
  onDuplicate,
  onDelete,
}: {
  model: Model;
  disabled: boolean;
  testing: boolean;
  result: CursorModelTestState | undefined;
  onTest: () => void;
  onEdit: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
}) {
  return (
    <div className={styles.modelRow}>
      <div className={styles.modelRowName}>
        <span className={styles.modelRowNameText}>{model.display_name}</span>
        <span className={styles.modelRowModelId}>{model.model_id}</span>
      </div>
      <CursorModelTestResult compact state={result} testing={testing} />
      <div className={styles.modelCardActions}>
        <TruncatedButton
          size="small"
          disabled={disabled && !testing}
          label={testing ? t("models.cancel_test") : t("models.test")}
          onClick={onTest}
        />
        <TruncatedButton size="small" disabled={disabled} label={t("common.edit")} onClick={onEdit} />
        <TruncatedButton
          size="small"
          disabled={disabled}
          label={t("models.duplicate")}
          onClick={onDuplicate}
        />
        <TruncatedButton
          size="small"
          className={styles.deleteButton}
          disabled={disabled}
          label={t("common.delete")}
          onClick={onDelete}
        />
      </div>
    </div>
  );
}

function PluginModelRow({
  model,
  disabled,
  testing,
  result,
  onTest,
  onSettings,
}: {
  model: PluginModelDescriptor;
  disabled: boolean;
  testing: boolean;
  result: CursorModelTestState | undefined;
  onTest: () => void;
  onSettings: () => void;
}) {
  return (
    <div className={styles.modelRow}>
      <div className={styles.modelRowName}>
        <span className={styles.modelRowNameText}>{model.displayName}</span>
        <span className={styles.modelRowModelId}>{model.modelId}</span>
      </div>
      <CursorModelTestResult compact state={result} testing={testing} />
      <div className={styles.modelCardActions}>
        <TruncatedButton
          size="small"
          disabled={disabled && !testing}
          label={testing ? t("models.cancel_test") : t("models.test")}
          onClick={onTest}
        />
        <TruncatedButton
          size="small"
          disabled={disabled}
          label={t("models.settings")}
          onClick={onSettings}
        />
      </div>
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
    try {
      return new URL(`https://${value}`).hostname.toLowerCase() || value;
    } catch {
      return value;
    }
  }
}

function typeGroup(model: Model) {
  if (model.type === "anthropic") return { key: "anthropic", label: "Anthropic", icon: claudeIcon };
  if (model.openai_endpoint === "/v1/chat/completions")
    return { key: "openai-chat", label: "OpenAI Chat", icon: openAiIcon };
  return { key: "openai-responses", label: "OpenAI Responses", icon: openAiIcon };
}
