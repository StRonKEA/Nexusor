import { useEffect, useMemo, useState } from "react";
import {
  api,
  pluginText,
  type AutoRouterConfig,
  type ComboSlot,
  type RouterCombo,
} from "../../shared/api";
import { PageContent } from "../../shell/layout/PageContent";
import { Card } from "../../shared/ui/Card";
import { Button } from "../../shared/ui/Button";
import { ConfirmDialog } from "../../shared/ui/ConfirmDialog";
import { Switch } from "../../shared/ui/Switch";
import { Icon } from "../../shared/ui/Icon";
import {
  boltIcon,
  codeIcon,
  brainIcon,
  speedIcon,
  gearIcon,
  emptyHexIcon,
  arrowRightIcon,
  skipForwardIcon,
  refreshIcon,
} from "../../shared/ui/icons";
import { useMessage } from "../../shared/ui/message";
import { useAppStore } from "../../shared/store/appStore";
import { getProviderLogo } from "../../shared/utils/providerIcons";
import { useI18n } from "../../i18n/store";
import { TaskEnginesTab } from "./TaskEnginesTab";
import { AutoRouterWizard } from "./AutoRouterWizard";
import { CustomComboWizard, type WizardState } from "./CustomComboWizard";
import styles from "./CombosPage.module.scss";

// ─── Domain types ─────────────────────────────────────────────────────────────

export interface Account {
  id: string;
  label: string;
  subLabel: string;
  status: "ready" | "cooling" | "invalid" | "disabled";
  icon: string | null;
  pluginId: string | null;
  resourceType: string | null;
  providerLabel: string;
  modelOptions: ModelOption[];
}

export interface ModelOption {
  id: string;
  label: string;
  icon: string | null;
  accountId: string;
  accountLabel: string;
  pluginId: string | null;
  images?: boolean;
}

/** Represents a concrete (account, model) slot in any chain */
export interface WizardSlot {
  /** Unique key: `${accountId}::${modelId}` */
  key: string;
  accountId: string;
  accountLabel: string;
  accountSubLabel: string;
  accountIcon: string | null;
  pluginId: string | null;
  modelId: string;
  modelLabel: string;
  modelIcon: string | null;
}

function freshWizard(): WizardState {
  return {
    combo_id: `combo-${Date.now()}`,
    selectedAccountIds: new Set(),
    selectedSlots: [],
    chainStrategy: "fallback",
    name: "",
    description: "",
  };
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function chainStrategyLabel(s: string) {
  if (s === "round_robin") return t("combos.chain_round_robin");
  if (s === "fill_first") return t("combos.chain_fill_first");
  if (s === "most_quota") return t("combos.chain_most_quota");
  return t("combos.chain_failover");
}

// ─── Main page ────────────────────────────────────────────────────────────────

export function CombosPage() {
  const message = useMessage();
  const { locale } = useI18n();
  const { models, plugins } = useAppStore();

  const [mainTab, setMainTab] = useState<"auto" | "chains" | "engines">("auto");
  const [combos, setCombos] = useState<RouterCombo[]>([]);
  const [deletingCombo, setDeletingCombo] = useState<RouterCombo | null>(null);
  const [autoConfig, setAutoConfig] = useState<AutoRouterConfig>({
    enabled: true,
    coding_slots: [],
    reasoning_slots: [],
    fast_slots: [],
    vision_auto: true,
    vision_slots: [],
    subagent_auto: true,
    subagent_slots: [],
    subagent_write_access: true,
    updated_at_ms: 0,
  });

  // Modal visibility and state
  const [comboWizardOpen, setComboWizardOpen] = useState(false);
  const [editingComboId, setEditingComboId] = useState<string | null>(null);
  const [activeComboWizard, setActiveComboWizard] = useState<WizardState>(freshWizard);
  const [autoWizardOpen, setAutoWizardOpen] = useState(false);

  const loadAll = async () => {
    try { setCombos(await api.routerCombos()); } catch (e) { message(String(e)); }
    try {
      const cfg = await api.autoRouterConfig();
      setAutoConfig(cfg);
    } catch {
      // default
    }
  };

  useEffect(() => { void loadAll(); }, [plugins.length]); // eslint-disable-line react-hooks/exhaustive-deps

  const toggleAutoSmart = async () => {
    const next = !autoConfig.enabled;
    try {
      const updated = { ...autoConfig, enabled: next, updated_at_ms: Date.now() };
      await api.saveAutoRouterConfig(updated);
      setAutoConfig(updated);
      await api.setModelCustomization("auto-smart", { custom_name: null, enabled: next });
      message(next ? t("combos.cursor_auto_mode_enabled") : t("combos.cursor_auto_mode_disabled"));
    } catch (e) {
      message(String(e));
    }
  };

  // ── Account catalogue ────────────────────────────────────────────────────
  const accounts = useMemo((): Account[] => {
    const result: Account[] = [];

    // Builtin models — each model IS its own "account"
    for (const m of models) {
      const icon = getProviderLogo(m.group_name) || getProviderLogo(m.base_url);
      result.push({
        id: m.model_hash,
        label: m.display_name,
        subLabel: m.group_name?.trim() || providerDomain(m.base_url),
        status: "ready",
        icon,
        pluginId: null,
        resourceType: null,
        providerLabel: m.group_name?.trim() || providerDomain(m.base_url),
        modelOptions: [{
          id: m.model_hash,
          label: m.display_name,
          icon,
          accountId: m.model_hash,
          accountLabel: m.display_name,
          pluginId: null,
        }],
      });
    }

    // Plugin OAuth resources
    for (const plugin of plugins) {
      const pluginIcon = getProviderLogo(plugin.id) || plugin.icon;
      for (const res of plugin.resources) {
        for (const resource of res.resources) {
          const status: Account["status"] =
            resource.state.status === "ready" ? "ready" :
            resource.state.status === "cooling" ? "cooling" :
            resource.state.status === "disabled" ? "disabled" : "invalid";

          const modelOptions: ModelOption[] = plugin.providers.flatMap((prov) =>
            prov.configured ? prov.models.map((m) => ({
              id: m.id,
              label: m.displayName,
              icon: pluginIcon,
              accountId: resource.id,
              accountLabel: resource.displayName || resource.id,
              pluginId: plugin.id,
              images: m.images,
            })) : []
          );

          result.push({
            id: resource.id,
            label: resource.displayName || resource.id,
            subLabel: pluginText(plugin.providers[0]?.displayName, locale) || plugin.name,
            status,
            icon: pluginIcon,
            pluginId: plugin.id,
            resourceType: res.type,
            providerLabel: plugin.name,
            modelOptions,
          });
        }
      }
    }
    return result;
  }, [models, plugins, locale]);

  // Account groups for Custom Combo
  const accountGroups = useMemo(() => {
    const map = new Map<string, { label: string; icon: string | null; accounts: Account[] }>();
    for (const acc of accounts) {
      const key = acc.providerLabel;
      if (!map.has(key)) map.set(key, { label: key, icon: acc.icon, accounts: [] });
      map.get(key)!.accounts.push(acc);
    }
    return [...map.values()];
  }, [accounts]);

  // Helper to map ComboSlot list to WizardSlot list
  const mapComboSlotsToWizard = (slots: ComboSlot[]): WizardSlot[] => {
    return slots.map((s) => {
      const aid = s.account_ids?.[0];
      const acc = aid
        ? accounts.find((a) => a.id === aid)
        : accounts.find((a) => a.id === s.model_id || a.modelOptions.some((m) => m.id === s.model_id));
      const mod = acc?.modelOptions.find((m) => m.id === s.model_id) ??
        accounts.flatMap((a) => a.modelOptions).find((m) => m.id === s.model_id);
      return {
        key: `${acc?.id ?? aid ?? "any"}::${s.model_id}`,
        accountId: acc?.id ?? aid ?? s.model_id,
        accountLabel: acc?.label ?? t("combos.models_label"),
        accountSubLabel: acc?.subLabel ?? "",
        accountIcon: acc?.icon ?? null,
        pluginId: acc?.pluginId ?? null,
        modelId: s.model_id,
        modelLabel: mod?.label ?? cleanModelName(s.model_id, accounts),
        modelIcon: mod?.icon || acc?.icon || null,
      };
    });
  };

  const openCreateCombo = () => {
    setActiveComboWizard(freshWizard());
    setEditingComboId(null);
    setComboWizardOpen(true);
  };

  const openEditCombo = (combo: RouterCombo) => {
    const accountIds = new Set<string>();
    const rawSlots = combo.slots && combo.slots.length > 0
      ? combo.slots
      : (combo.models || []).map((mid) => ({
          model_id: mid,
          account_ids: null,
          slot_strategy: combo.strategy,
        }));

    const slots = mapComboSlotsToWizard(rawSlots);
    for (const s of slots) accountIds.add(s.accountId);

    setActiveComboWizard({
      combo_id: combo.combo_id,
      selectedAccountIds: accountIds,
      selectedSlots: slots,
      chainStrategy:
        combo.strategy === "round_robin"
          ? "round_robin"
          : combo.strategy === "fill_first"
          ? "fill_first"
          : combo.strategy === "most_quota"
          ? "most_quota"
          : "fallback",
      name: combo.name,
      description: combo.description ?? "",
    });
    setEditingComboId(combo.combo_id);
    setComboWizardOpen(true);
  };

  const handleDeleteCombo = async (id: string) => {
    try { await api.deleteRouterCombo(id); await loadAll(); message(t("combos.combo_deleted")); }
    catch (e) { message(String(e)); }
  };

  const handleToggleCombo = async (combo: RouterCombo) => {
    try { await api.upsertRouterCombo({ ...combo, enabled: !combo.enabled, updated_at_ms: Date.now() }); await loadAll(); }
    catch (e) { message(String(e)); }
  };

  const pageContent = (
    <div className={styles.page}>
      <div className={styles.mainTabBar}>
        <button
          type="button"
          className={`${styles.mainTabButton} ${mainTab === "auto" ? styles.mainTabActive : ""}`}
          onClick={() => setMainTab("auto")}
        >
          <span>{t("combos.tab_auto")}</span>
        </button>
        <button
          type="button"
          className={`${styles.mainTabButton} ${mainTab === "chains" ? styles.mainTabActive : ""}`}
          onClick={() => setMainTab("chains")}
        >
          <span>{t("combos.tab_chains", { count: combos.length })}</span>
        </button>
        <button
          type="button"
          className={`${styles.mainTabButton} ${mainTab === "engines" ? styles.mainTabActive : ""}`}
          onClick={() => setMainTab("engines")}
        >
          <span>{t("combos.tab_engines")}</span>
        </button>
      </div>

      {mainTab === "auto" ? (
        <Card className={styles.autoCard}>
          <div className={styles.autoCardTop}>
            <div className={styles.autoCardTitleGroup}>
              <span className={styles.autoCardIcon}><Icon icon={boltIcon} size="1.2em" /></span>
              <div>
                <h3 className={styles.autoCardTitle}>
                  {t("combos.cursor_auto_mode_smart_router")}
                </h3>
                <p className={styles.autoCardDesc}>
                  {t("combos.when_auto_is_selected_in_the_cur")}
                </p>
              </div>
            </div>
            <div className={styles.autoActions}>
              <Button variant="secondary" size="small" onClick={() => setAutoWizardOpen(true)}>
                <span className={styles.inlineButtonContent}>
                  <Icon icon={gearIcon} size="1.1em" /> {t("combos.configure_auto")}
                </span>
              </Button>
              <Switch
                checked={autoConfig.enabled}
                onChange={() => void toggleAutoSmart()}
                label={autoConfig.enabled ? t("combos.enabled_on") : t("combos.disabled_off")}
              />
            </div>
          </div>

          {/* Live Category Summary Grid */}
          <div className={styles.autoCategoriesGrid}>
            {/* Coding */}
            <div className={styles.autoCategoryCard}>
              <div className={styles.autoCatHeader}>
                <span className={styles.autoCatIconSvg}><Icon icon={codeIcon} size="1.1em" /></span>
                <span>{t("combos.auto_step_coding")}</span>
              </div>
              <div className={styles.autoCatSlots}>
                {autoConfig.coding_slots.length === 0 ? (
                  <span className={styles.autoCatEmpty}>{t("combos.no_slots_in_category")}</span>
                ) : (
                  autoConfig.coding_slots.map((s, idx) => {
                    const name = cleanModelName(s.model_id, accounts);
                    const acc = getSlotAccount(s, accounts);
                    const icon = acc?.modelOptions.find((m) => m.id === s.model_id)?.icon || acc?.icon;
                    return (
                      <div key={idx} className={styles.autoCatSlotItem}>
                        <span className={styles.autoCatSlotIdx}>{idx + 1}.</span>
                        {icon && <img src={icon} alt="" className={styles.autoCatSlotIcon} />}
                        <span className={styles.autoCatSlotName}>{name}</span>
                        {acc && <span className={styles.autoCatSlotAccount}>{acc.label}</span>}
                      </div>
                    );
                  })
                )}
              </div>
            </div>

            {/* Reasoning */}
            <div className={styles.autoCategoryCard}>
              <div className={styles.autoCatHeader}>
                <span className={styles.autoCatIconSvg}><Icon icon={brainIcon} size="1.1em" /></span>
                <span>{t("combos.auto_step_reasoning")}</span>
              </div>
              <div className={styles.autoCatSlots}>
                {autoConfig.reasoning_slots.length === 0 ? (
                  <span className={styles.autoCatEmpty}>{t("combos.no_slots_in_category")}</span>
                ) : (
                  autoConfig.reasoning_slots.map((s, idx) => {
                    const name = cleanModelName(s.model_id, accounts);
                    const acc = getSlotAccount(s, accounts);
                    const icon = acc?.modelOptions.find((m) => m.id === s.model_id)?.icon || acc?.icon;
                    return (
                      <div key={idx} className={styles.autoCatSlotItem}>
                        <span className={styles.autoCatSlotIdx}>{idx + 1}.</span>
                        {icon && <img src={icon} alt="" className={styles.autoCatSlotIcon} />}
                        <span className={styles.autoCatSlotName}>{name}</span>
                        {acc && <span className={styles.autoCatSlotAccount}>{acc.label}</span>}
                      </div>
                    );
                  })
                )}
              </div>
            </div>

            {/* Fast */}
            <div className={styles.autoCategoryCard}>
              <div className={styles.autoCatHeader}>
                <span className={styles.autoCatIconSvg}><Icon icon={speedIcon} size="1.1em" /></span>
                <span>{t("combos.auto_step_fast")}</span>
              </div>
              <div className={styles.autoCatSlots}>
                {autoConfig.fast_slots.length === 0 ? (
                  <span className={styles.autoCatEmpty}>{t("combos.no_slots_in_category")}</span>
                ) : (
                  autoConfig.fast_slots.map((s, idx) => {
                    const name = cleanModelName(s.model_id, accounts);
                    const acc = getSlotAccount(s, accounts);
                    const icon = acc?.modelOptions.find((m) => m.id === s.model_id)?.icon || acc?.icon;
                    return (
                      <div key={idx} className={styles.autoCatSlotItem}>
                        <span className={styles.autoCatSlotIdx}>{idx + 1}.</span>
                        {icon && <img src={icon} alt="" className={styles.autoCatSlotIcon} />}
                        <span className={styles.autoCatSlotName}>{name}</span>
                        {acc && <span className={styles.autoCatSlotAccount}>{acc.label}</span>}
                      </div>
                    );
                  })
                )}
              </div>
            </div>
          </div>
        </Card>
      ) : mainTab === "chains" ? (
        <div className={styles.customCombosSection}>
          <div className={styles.customCombosHeader}>
            <div className={styles.customCombosTitleGroup}>
              <h3 className={styles.customCombosTitle}>{t("combos.custom_combos_title")}</h3>
              <p className={styles.customCombosDesc}>{t("combos.custom_combos_desc")}</p>
            </div>
            <Button variant="primary" size="medium" onClick={openCreateCombo}>
              + {t("combos.new_combo")}
            </Button>
          </div>

          {combos.length === 0 ? (
            <EmptyState onAdd={openCreateCombo} />
          ) : (
            combos.map((combo) => (
              <ComboCard
                key={combo.combo_id}
                combo={combo}
                accounts={accounts}
                onEdit={() => openEditCombo(combo)}
                onDelete={() => setDeletingCombo(combo)}
                onToggle={() => void handleToggleCombo(combo)}
              />
            ))
          )}
        </div>
      ) : (
        <TaskEnginesTab
          autoConfig={autoConfig}
          onSaveConfig={async (cfg) => {
            await api.saveAutoRouterConfig(cfg);
            setAutoConfig(cfg);
          }}
          accounts={accounts}
          cleanModelName={cleanModelName}
          getSlotAccount={getSlotAccount}
        />
      )}
    </div>
  );

  return (
    <>
      <PageContent
        title={t("combos.combos")}
        sections={[{ key: "combos", estimatedHeight: 600, content: pageContent }]}
      />

      <ConfirmDialog
        open={deletingCombo !== null}
        title={t("combos.delete_combo_confirm_title")}
        cancelLabel={t("common.cancel")}
        confirmLabel={t("common.delete")}
        onCancel={() => setDeletingCombo(null)}
        onConfirm={() => {
          if (deletingCombo) {
            void handleDeleteCombo(deletingCombo.combo_id);
            setDeletingCombo(null);
          }
        }}
      >
        <p>
          {t("combos.delete_combo_confirm_desc", { name: deletingCombo?.name || "" })}
        </p>
      </ConfirmDialog>

      {/* Auto Router Wizard Modal */}
      {autoWizardOpen && (
        <AutoRouterWizard
          open={autoWizardOpen}
          onClose={() => setAutoWizardOpen(false)}
          autoConfig={autoConfig}
          onConfigSaved={setAutoConfig}
          accounts={accounts}
          cleanModelName={cleanModelName}
        />
      )}

      {/* Custom Combo Wizard Modal */}
      {comboWizardOpen && (
        <CustomComboWizard
          open={comboWizardOpen}
          onClose={() => setComboWizardOpen(false)}
          editingComboId={editingComboId}
          initialWizard={activeComboWizard}
          combos={combos}
          onComboSaved={loadAll}
          accounts={accounts}
          accountGroups={accountGroups}
        />
      )}
    </>
  );
}

// ─── Empty state ──────────────────────────────────────────────────────────────
function EmptyState({ onAdd }: { onAdd: () => void }) {
  return (
    <div className={styles.emptyState}>
      <div className={styles.emptyIcon}><Icon icon={emptyHexIcon} size="2.5em" /></div>
      <h3 className={styles.emptyTitle}>{t("combos.no_combos_yet")}</h3>
      <p className={styles.emptyDesc}>{t("combos.empty_desc")}</p>
      <Button variant="primary" size="medium" onClick={onAdd}>
        + {t("combos.add_first_combo")}
      </Button>
    </div>
  );
}

// ─── Combo card ───────────────────────────────────────────────────────────────
function ComboCard({ combo, accounts, onEdit, onDelete, onToggle }: {
  combo: RouterCombo;
  accounts: Account[];
  onEdit: () => void;
  onDelete: () => void;
  onToggle: () => void;
}) {
  const allModelOptions = accounts.flatMap((a) => a.modelOptions);

  // Group consecutive identical model slots into a pooled representation
  const groupedSlots = useMemo(() => {
    interface GroupedSlotItem {
      modelId: string;
      label: string;
      icon: string | null;
      accounts: Account[];
      count: number;
    }
    const groups: GroupedSlotItem[] = [];
    for (const slot of combo.slots) {
      const aid = slot.account_ids?.[0];
      const acc = aid
        ? accounts.find((a) => a.id === aid)
        : accounts.find((a) => a.id === slot.model_id || a.modelOptions.some((m) => m.id === slot.model_id));
      const m = acc?.modelOptions.find((o) => o.id === slot.model_id) ??
        allModelOptions.find((o) => o.id === slot.model_id);
      const icon = m?.icon || acc?.icon || null;
      const label = m?.label ?? cleanModelName(slot.model_id, accounts);

      const last = groups[groups.length - 1];
      if (last && last.modelId === slot.model_id) {
        last.count += 1;
        if (acc && !last.accounts.some((a) => a.id === acc.id)) {
          last.accounts.push(acc);
        }
      } else {
        groups.push({
          modelId: slot.model_id,
          label,
          icon,
          accounts: acc ? [acc] : [],
          count: 1,
        });
      }
    }
    return groups;
  }, [combo.slots, accounts, allModelOptions]);

  return (
    <Card className={styles.comboCard}>
      <div className={styles.comboCardTop}>
        <div className={styles.comboCardMeta}>
          <span className={`${styles.dot} ${combo.enabled ? styles.dotActive : styles.dotOff}`} />
          <h3 className={styles.comboName}>{combo.name}</h3>
          {!combo.enabled && <span className={styles.disabledTag}>{t("combos.disabled")}</span>}
          <span className={styles.comboBadge}>{chainStrategyLabel(combo.strategy, t)}</span>
        </div>
        <div className={styles.comboCardActions}>
          <Button size="small" variant="secondary" onClick={onToggle}>
            {combo.enabled ? t("combos.disable") : t("combos.enabled")}
          </Button>
          <Button size="small" variant="secondary" onClick={onEdit}>{t("common.edit")}</Button>
          <Button size="small" variant="secondary" onClick={onDelete}>{t("common.delete")}</Button>
        </div>
      </div>
      {combo.description && <p className={styles.comboDesc}>{combo.description}</p>}
      <div className={styles.slotReadList}>
        {groupedSlots.map((group, idx) => (
          <div key={idx} className={styles.slotReadGroup}>
            <div className={styles.slotRead}>
              <span className={styles.slotReadIndex}>{idx + 1}</span>
              {group.icon && <img src={group.icon} alt="" className={styles.slotReadIcon} />}
              <span className={styles.slotReadName}>{group.label}</span>
              {group.count > 1 ? (
                <span className={styles.slotReadAccount}>
                  {t("combos.account_pool_format", { count: group.count, accounts: group.accounts.map((a) => a.label).join(", ") })}
                </span>
              ) : group.accounts[0] ? (
                <span className={styles.slotReadAccount}>
                  {group.accounts[0].label}
                </span>
              ) : null}
            </div>
            {idx < groupedSlots.length - 1 && (
              <span className={styles.slotArrow}>
                <Icon icon={combo.strategy === "round_robin" ? refreshIcon : combo.strategy === "fill_first" ? skipForwardIcon : arrowRightIcon} size="1em" />
              </span>
            )}
          </div>
        ))}
      </div>
    </Card>
  );
}

// ─── Util ─────────────────────────────────────────────────────────────────────
function cleanModelName(modelId: string, accounts: Account[]): string {
  if (!modelId) return "";
  for (const acc of accounts) {
    const m = acc.modelOptions.find((opt) => opt.id === modelId);
    if (m?.label) return m.label;
  }
  if (modelId.startsWith("plugin:")) {
    const lastSlash = modelId.lastIndexOf("/");
    if (lastSlash !== -1 && lastSlash < modelId.length - 1) {
      return modelId.substring(lastSlash + 1);
    }
  }
  return modelId;
}

function getSlotAccount(slot: ComboSlot, accounts: Account[]): Account | undefined {
  const aid = slot.account_ids?.[0];
  if (aid) {
    const acc = accounts.find((a) => a.id === aid);
    if (acc) return acc;
  }
  return accounts.find((a) => a.id === slot.model_id || a.modelOptions.some((m) => m.id === slot.model_id));
}

function providerDomain(baseUrl: string) {
  try { return new URL(baseUrl).hostname.replace(/^www\./, ""); }
  catch { try { return new URL(`https://${baseUrl}`).hostname; } catch { return baseUrl; } }
}
