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
import { Modal } from "../../shared/ui/Modal";
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
} from "../../shared/ui/icons";
import { useMessage } from "../../shared/ui/message";
import { useAppStore } from "../../shared/store/appStore";
import { getProviderLogo } from "../../shared/utils/providerIcons";
import { useI18n } from "../../i18n/store";
import { TaskEnginesTab } from "./TaskEnginesTab";
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
interface WizardSlot {
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

// ─── Custom Combo Wizard State ───────────────────────────────────────────────

type WizardStep = 1 | 2 | 3 | 4;

interface WizardState {
  combo_id: string;
  selectedAccountIds: Set<string>;
  selectedSlots: WizardSlot[];
  chainStrategy: "fallback" | "round_robin" | "fill_first" | "most_quota";
  name: string;
  description: string;
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

// ─── Auto Router Wizard State ─────────────────────────────────────────────────

interface AutoWizardState {
  codingSlots: WizardSlot[];
  reasoningSlots: WizardSlot[];
  fastSlots: WizardSlot[];
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function statusClass(status: Account["status"]) {
  if (status === "ready") return styles.statusReady;
  if (status === "cooling") return styles.statusCooling;
  if (status === "disabled") return styles.statusDisabled;
  return styles.statusInvalid;
}

function statusDot(_status: Account["status"]) {
  return "●";
}

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

  // Custom combo modal state
  const [comboWizardOpen, setComboWizardOpen] = useState(false);
  const [comboStep, setComboStep] = useState<WizardStep>(1);
  const [comboWizard, setComboWizard] = useState<WizardState>(freshWizard);
  const [comboSearch, setComboSearch] = useState("");
  const [editingComboId, setEditingComboId] = useState<string | null>(null);
  const [comboSaving, setComboSaving] = useState(false);

  // Auto Router modal state
  const [autoWizardOpen, setAutoWizardOpen] = useState(false);
  const [autoWizardStep, setAutoWizardStep] = useState<1 | 2 | 3 | 4>(1);
  const [autoWizard, setAutoWizard] = useState<AutoWizardState>({
    codingSlots: [],
    reasoningSlots: [],
    fastSlots: [],
  });
  const [autoSearch, setAutoSearch] = useState("");
  const [autoSaving, setAutoSaving] = useState(false);

  const loadAll = async () => {
    try { setCombos(await api.routerCombos()); } catch (e) { message(String(e)); }
    try {
      const cfg = await api.autoRouterConfig();
      setAutoConfig(cfg);
    } catch {
      // default
    }
  };

  // Refetch when the configured plugin set changes. `useMemo` would run during
  // render and race with the store snapshot it depends on.
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

  // Account groups for Custom Combo Step 1
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
        accountLabel: acc?.label ?? "Hesap",
        accountSubLabel: acc?.subLabel ?? "",
        accountIcon: acc?.icon ?? null,
        pluginId: acc?.pluginId ?? null,
        modelId: s.model_id,
        modelLabel: mod?.label ?? cleanModelName(s.model_id, accounts),
        modelIcon: mod?.icon || acc?.icon || null,
      };
    });
  };

  // ── Auto Router Wizard handlers ───────────────────────────────────────────
  const openAutoWizard = () => {
    setAutoWizard({
      codingSlots: mapComboSlotsToWizard(autoConfig.coding_slots),
      reasoningSlots: mapComboSlotsToWizard(autoConfig.reasoning_slots),
      fastSlots: mapComboSlotsToWizard(autoConfig.fast_slots),
    });
    setAutoWizardStep(1);
    setAutoSearch("");
    setAutoWizardOpen(true);
  };

  const saveAutoWizard = async () => {
    setAutoSaving(true);
    try {
      const toComboSlots = (slots: WizardSlot[]): ComboSlot[] => {
        return slots.map((s) => ({
          model_id: s.modelId,
          account_ids: s.pluginId ? [s.accountId] : null,
          slot_strategy: "fallback",
        }));
      };

      const newConfig: AutoRouterConfig = {
        ...autoConfig,
        coding_slots: toComboSlots(autoWizard.codingSlots),
        reasoning_slots: toComboSlots(autoWizard.reasoningSlots),
        fast_slots: toComboSlots(autoWizard.fastSlots),
        updated_at_ms: Date.now(),
      };

      await api.saveAutoRouterConfig(newConfig);
      setAutoConfig(newConfig);
      setAutoWizardOpen(false);
      message(t("combos.auto_config_saved"));
    } catch (e) {
      message(String(e));
    } finally {
      setAutoSaving(false);
    }
  };

  const getAutoStepSlots = (step: 1 | 2 | 3): WizardSlot[] => {
    if (step === 1) return autoWizard.codingSlots;
    if (step === 2) return autoWizard.reasoningSlots;
    return autoWizard.fastSlots;
  };

  const toggleAutoSlot = (step: 1 | 2 | 3, account: Account, model: ModelOption) => {
    const key = `${account.id}::${model.id}`;
    setAutoWizard((w) => {
      const prop = step === 1 ? "codingSlots" : step === 2 ? "reasoningSlots" : "fastSlots";
      const current = w[prop];
      const exists = current.some((s) => s.key === key);
      const updated = exists
        ? current.filter((s) => s.key !== key)
        : [
            ...current,
            {
              key,
              accountId: account.id,
              accountLabel: account.label,
              accountSubLabel: account.subLabel,
              accountIcon: account.icon,
              pluginId: account.pluginId,
              modelId: model.id,
              modelLabel: model.label,
              modelIcon: model.icon || account.icon,
            },
          ];
      return { ...w, [prop]: updated };
    });
  };

  const moveAutoSlot = (step: 1 | 2 | 3, index: number, delta: -1 | 1) => {
    setAutoWizard((w) => {
      const prop = step === 1 ? "codingSlots" : step === 2 ? "reasoningSlots" : "fastSlots";
      const target = index + delta;
      const current = w[prop];
      if (target < 0 || target >= current.length) return w;
      const next = [...current];
      [next[index], next[target]] = [next[target], next[index]];
      return { ...w, [prop]: next };
    });
  };

  const removeAutoSlot = (step: 1 | 2 | 3, index: number) => {
    setAutoWizard((w) => {
      const prop = step === 1 ? "codingSlots" : step === 2 ? "reasoningSlots" : "fastSlots";
      const current = w[prop];
      return { ...w, [prop]: current.filter((_, i) => i !== index) };
    });
  };

  // Filtered accounts for Auto Wizard
  const filteredAccountsForAuto = useMemo(() => {
    const q = autoSearch.trim().toLowerCase();
    return accounts
      .map((acc) => {
        if (!q) return acc;
        const accMatch =
          acc.label.toLowerCase().includes(q) ||
          acc.subLabel.toLowerCase().includes(q) ||
          acc.providerLabel.toLowerCase().includes(q);
        const matchedMods = acc.modelOptions.filter(
          (m) =>
            accMatch ||
            m.label.toLowerCase().includes(q) ||
            m.id.toLowerCase().includes(q)
        );
        return { ...acc, modelOptions: matchedMods };
      })
      .filter((a) => a.modelOptions.length > 0);
  }, [accounts, autoSearch]);

  // ── Custom Combo Wizard handlers ──────────────────────────────────────────
  const modelsByAccount = useMemo(() => {
    return accounts
      .filter((a) => comboWizard.selectedAccountIds.has(a.id))
      .map((a) => ({ account: a, models: a.modelOptions }))
      .filter((g) => g.models.length > 0);
  }, [accounts, comboWizard.selectedAccountIds]);

  const filteredModelsByAccount = useMemo(() => {
    const q = comboSearch.trim().toLowerCase();
    if (!q) return modelsByAccount;
    return modelsByAccount
      .map(({ account, models: mods }) => {
        const accMatch =
          account.label.toLowerCase().includes(q) ||
          account.subLabel.toLowerCase().includes(q) ||
          account.providerLabel.toLowerCase().includes(q);
        const matchedMods = mods.filter(
          (m) =>
            accMatch ||
            m.label.toLowerCase().includes(q) ||
            m.id.toLowerCase().includes(q)
        );
        return { account, models: matchedMods };
      })
      .filter((g) => g.models.length > 0);
  }, [modelsByAccount, comboSearch]);

  const openCreateCombo = () => {
    setComboWizard(freshWizard());
    setEditingComboId(null);
    setComboSearch("");
    setComboStep(1);
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

    setComboWizard({
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
    setComboSearch("");
    setComboStep(1);
    setComboWizardOpen(true);
  };

  const handleSaveCombo = async () => {
    if (!comboWizard.name.trim()) {
      message(t("combos.please_enter_a_name_for_the_comb"));
      return;
    }
    setComboSaving(true);
    try {
      const existing = combos.find((c) => c.combo_id === comboWizard.combo_id);
      const slots: ComboSlot[] = comboWizard.selectedSlots.map((s) => ({
        model_id: s.modelId,
        account_ids: s.pluginId ? [s.accountId] : null,
        slot_strategy: comboWizard.chainStrategy,
      }));

      await api.upsertRouterCombo({
        combo_id: comboWizard.combo_id,
        name: comboWizard.name.trim(),
        description: comboWizard.description.trim() || null,
        strategy: comboWizard.chainStrategy,
        models: slots.map((s) => s.model_id),
        slots,
        enabled: existing?.enabled ?? true,
        created_at_ms: existing?.created_at_ms ?? Date.now(),
        updated_at_ms: Date.now(),
      });

      setComboWizardOpen(false);
      await loadAll();
      message(editingComboId ? t("combos.combo_updated") : t("combos.smart_combo_created_successfully"));
    } catch (e) {
      message(String(e));
    } finally {
      setComboSaving(false);
    }
  };

  const handleDeleteCombo = async (id: string) => {
    try { await api.deleteRouterCombo(id); await loadAll(); message(t("combos.combo_deleted")); }
    catch (e) { message(String(e)); }
  };

  const handleToggleCombo = async (combo: RouterCombo) => {
    try { await api.upsertRouterCombo({ ...combo, enabled: !combo.enabled, updated_at_ms: Date.now() }); await loadAll(); }
    catch (e) { message(String(e)); }
  };

  const toggleAccount = (id: string) => setComboWizard((w) => {
    const next = new Set(w.selectedAccountIds);
    if (next.has(id)) {
      next.delete(id);
      return {
        ...w,
        selectedAccountIds: next,
        selectedSlots: w.selectedSlots.filter((s) => s.accountId !== id),
      };
    } else {
      next.add(id);
      return { ...w, selectedAccountIds: next };
    }
  });

  const toggleComboSlot = (account: Account, model: ModelOption) => {
    const key = `${account.id}::${model.id}`;
    setComboWizard((w) => {
      const exists = w.selectedSlots.some((s) => s.key === key);
      if (exists) {
        return {
          ...w,
          selectedSlots: w.selectedSlots.filter((s) => s.key !== key),
        };
      } else {
        const newSlot: WizardSlot = {
          key,
          accountId: account.id,
          accountLabel: account.label,
          accountSubLabel: account.subLabel,
          accountIcon: account.icon,
          pluginId: account.pluginId,
          modelId: model.id,
          modelLabel: model.label,
          modelIcon: model.icon || account.icon,
        };
        return {
          ...w,
          selectedSlots: [...w.selectedSlots, newSlot],
        };
      }
    });
  };

  const moveComboSlot = (index: number, delta: -1 | 1) => {
    setComboWizard((w) => {
      const target = index + delta;
      if (target < 0 || target >= w.selectedSlots.length) return w;
      const next = [...w.selectedSlots];
      [next[index], next[target]] = [next[target], next[index]];
      return { ...w, selectedSlots: next };
    });
  };

  // ── Render Page ───────────────────────────────────────────────────────────
  const stepLabels = ["", t("combos.step_accounts"), t("combos.step_models"), t("combos.step_order"), t("combos.step_summary")];
  const autoStepLabels = ["", t("combos.auto_step_coding"), t("combos.auto_step_reasoning"), t("combos.auto_step_fast"), t("combos.auto_step_summary")];

  const pageContent = (
    <div className={styles.page}>
      <div className={styles.mainTabBar}>
        <button
          type="button"
          className={`${styles.mainTabButton} ${mainTab === "auto" ? styles.mainTabActive : ""}`}
          onClick={() => setMainTab("auto")}
        >
          <span>Akıllı Yönlendirici (Auto Router)</span>
        </button>
        <button
          type="button"
          className={`${styles.mainTabButton} ${mainTab === "chains" ? styles.mainTabActive : ""}`}
          onClick={() => setMainTab("chains")}
        >
          <span>Özel Model Zincirleri ({combos.length})</span>
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
              <Button variant="secondary" size="small" onClick={openAutoWizard}>
                <span style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
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

      {/* ── Modal 1: Auto Mode Setup Wizard ── */}
      <Modal
        open={autoWizardOpen}
        title={t("combos.auto_wizard_title")}
        busy={autoSaving}
        onClose={() => setAutoWizardOpen(false)}
      >
        <div className={styles.wizardWrap}>
          {/* Progress strip */}
          <div className={styles.wizardProgress}>
            {([1, 2, 3, 4] as const).map((s) => (
              <div
                key={s}
                className={`${styles.wizardDot} ${s <= autoWizardStep ? styles.wizardDotActive : ""}`}
                title={autoStepLabels[s]}
              />
            ))}
            <span className={styles.wizardStepLabel}>{autoStepLabels[autoWizardStep]}</span>
          </div>

          <div className={styles.wizardScroll}>
            {/* Steps 1, 2, 3: Category Configuration */}
            {(autoWizardStep === 1 || autoWizardStep === 2 || autoWizardStep === 3) && (() => {
              const step = autoWizardStep as 1 | 2 | 3;
              const slots = getAutoStepSlots(step);
              const info =
                step === 1
                  ? {
                      title: t("combos.auto_step_coding"),
                      icon: codeIcon,
                      desc: t("combos.auto_coding_desc"),
                      chips: [
                        "Claude Fable 5",
                        "GPT-5.6 Sol",
                        "GPT-5.5",
                        "DeepSeek-V4-Pro (1.6T)",
                        "Kimi K3 (2.8T)",
                        "Claude Sonnet 5",
                      ],
                    }
                  : step === 2
                  ? {
                      title: t("combos.auto_step_reasoning"),
                      icon: brainIcon,
                      desc: t("combos.auto_reasoning_desc"),
                      chips: [
                        "GLM-5.2 (753B)",
                        "Kimi K2.6 (1T)",
                        "Claude Fable 5",
                        "GPT-5.6 Sol",
                        "DeepSeek-V4-Pro",
                        "DeepSeek-R1",
                      ],
                    }
                  : {
                      title: t("combos.auto_step_fast"),
                      icon: speedIcon,
                      desc: t("combos.auto_fast_desc"),
                      chips: [
                        "DeepSeek-V4-Flash (284B)",
                        "Qwen3.6-Plus / 27B",
                        "Gemini 3.8 Flash",
                        "MiniMax M3",
                        "Groq Llama 3.3 70B",
                      ],
                    };

              return (
                <div className={styles.stepRoot}>
                  {/* Step Category Title */}
                  <div className={styles.autoStepHeader}>
                    <div className={styles.autoCatHeader}>
                      <span className={styles.autoCatIconSvg}><Icon icon={info.icon} size="1.1em" /></span>
                      <span style={{ fontSize: "0.95rem" }}>{info.title}</span>
                    </div>
                  </div>
                  {/* Informative Guidance Banner */}
                  <div className={styles.autoInfoBanner}>
                    <p className={styles.autoInfoText}>{info.desc}</p>
                    <div className={styles.autoExampleRow}>
                      <span className={styles.autoExampleLabel}>{t("combos.example_models")}</span>
                      {info.chips.map((chip) => (
                        <span key={chip} className={styles.autoExampleChip}>{chip}</span>
                      ))}
                    </div>
                  </div>

                  {/* Selected Slots list for this category with reordering */}
                  {slots.length > 0 && (
                    <div className={styles.selectedSlotsSection}>
                      <span className={styles.selectedSlotsHeader}>
                        {t("combos.attempt_order")} ({slots.length} model)
                      </span>
                      <div className={styles.slotList}>
                        {slots.map((slot, idx) => (
                          <div key={slot.key} className={styles.slot}>
                            <span className={styles.slotIndex}>{idx + 1}</span>
                            {slot.modelIcon && <img src={slot.modelIcon} alt="" className={styles.slotIcon} />}
                            <div className={styles.slotInfo}>
                              <span className={styles.slotName}>{slot.modelLabel}</span>
                              <span className={styles.slotAccountBadge}>
                                {slot.accountLabel}
                                {slot.accountSubLabel && ` · ${slot.accountSubLabel}`}
                              </span>
                            </div>
                            <div className={styles.slotMoves}>
                              <button
                                className={styles.slotMoveBtn}
                                disabled={idx === 0}
                                onClick={() => moveAutoSlot(step, idx, -1)}
                                title={t("combos.move_up")}
                              >↑</button>
                              <button
                                className={styles.slotMoveBtn}
                                disabled={idx === slots.length - 1}
                                onClick={() => moveAutoSlot(step, idx, 1)}
                                title={t("combos.move_down")}
                              >↓</button>
                            </div>
                            <button
                              className={styles.slotRemoveBtn}
                              onClick={() => removeAutoSlot(step, idx)}
                              title={t("combos.remove_from_list")}
                            >✕</button>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}

                  {/* Search Bar */}
                  <div className={styles.searchBar}>
                    <input
                      type="text"
                      className={styles.searchInput}
                      placeholder={t("combos.search_models_or_accounts")}
                      value={autoSearch}
                      onChange={(e) => setAutoSearch(e.target.value)}
                    />
                    {autoSearch && (
                      <button
                        className={styles.searchClearBtn}
                        onClick={() => setAutoSearch("")}
                        type="button"
                        title={t("common.clear")}
                      >
                        ✕
                      </button>
                    )}
                  </div>

                  {/* Account grouped models list */}
                  {filteredAccountsForAuto.map((acc) => (
                    <div key={acc.id} className={styles.accountGroup}>
                      <div className={styles.accountGroupHeader}>
                        {acc.icon && <img src={acc.icon} alt="" className={styles.groupIcon} />}
                        <div className={styles.accountHeaderInfo}>
                          <span>{acc.label}</span>
                          <span className={styles.accountHeaderSub}>{acc.subLabel}</span>
                        </div>
                        <span className={`${styles.accountStatus} ${statusClass(acc.status)}`}>
                          {statusDot(acc.status)}
                        </span>
                      </div>
                      {acc.modelOptions.map((m) => {
                        const selIdx = slots.findIndex((s) => s.key === `${acc.id}::${m.id}`);
                        const isSel = selIdx >= 0;
                        return (
                          <label key={m.id} className={`${styles.modelRow} ${isSel ? styles.modelRowSel : ""}`}>
                            <input
                              type="checkbox"
                              checked={isSel}
                              onChange={() => toggleAutoSlot(step, acc, m)}
                            />
                            <span className={styles.modelLabel}>{m.label}</span>
                            {isSel && (
                              <span className={styles.selectedBadge}>
                                ✓ {selIdx + 1}.
                              </span>
                            )}
                          </label>
                        );
                      })}
                    </div>
                  ))}
                </div>
              );
            })()}

            {/* Step 4: Summary of all 3 categories */}
            {autoWizardStep === 4 && (
              <div className={styles.stepRoot}>
                <div className={styles.summaryBox}>
                  <div className={styles.summaryHeader}>
                    <span className={styles.summaryTitle}>{t("combos.summary")}</span>
                    <span className={styles.summaryMeta}>{t("combos.cursor_auto_mode_smart_router")}</span>
                  </div>

                  {/* 1. Coding Summary */}
                  <div className={styles.summaryCategorySection}>
                    <div className={styles.summaryCategoryTitle}>
                      <span className={styles.autoCatIconSvg}><Icon icon={codeIcon} size="1.1em" /></span>
                      <span>{t("combos.auto_step_coding")}</span>
                    </div>
                    {autoWizard.codingSlots.length === 0 ? (
                      <span className={styles.autoCatEmpty}>{t("combos.no_slots_in_category")}</span>
                    ) : (
                      <div className={styles.summaryVerticalList}>
                        {autoWizard.codingSlots.map((slot, i) => (
                          <div key={slot.key} className={styles.summaryItemWrapper}>
                            <div className={styles.summaryRow}>
                              <span className={styles.summaryRowIndex}>{i + 1}</span>
                              {slot.modelIcon && <img src={slot.modelIcon} alt="" className={styles.summaryRowIcon} />}
                              <span className={styles.summaryRowModel}>{slot.modelLabel}</span>
                              <span className={styles.summaryRowAccount}>
                                {slot.accountLabel}
                                {slot.accountSubLabel && ` · ${slot.accountSubLabel}`}
                              </span>
                            </div>
                            {i < autoWizard.codingSlots.length - 1 && (
                              <div className={styles.summaryConnector}>
                                <div className={styles.summaryConnectorLine} />
                                <span className={styles.summaryConnectorLabel}>
                                  ↓ {t("combos.failover_to_next")}
                                </span>
                              </div>
                            )}
                          </div>
                        ))}
                      </div>
                    )}
                  </div>

                  {/* 2. Reasoning Summary */}
                  <div className={styles.summaryCategorySection}>
                    <div className={styles.summaryCategoryTitle}>
                      <span className={styles.autoCatIconSvg}><Icon icon={brainIcon} size="1.1em" /></span>
                      <span>{t("combos.auto_step_reasoning")}</span>
                    </div>
                    {autoWizard.reasoningSlots.length === 0 ? (
                      <span className={styles.autoCatEmpty}>{t("combos.no_slots_in_category")}</span>
                    ) : (
                      <div className={styles.summaryVerticalList}>
                        {autoWizard.reasoningSlots.map((slot, i) => (
                          <div key={slot.key} className={styles.summaryItemWrapper}>
                            <div className={styles.summaryRow}>
                              <span className={styles.summaryRowIndex}>{i + 1}</span>
                              {slot.modelIcon && <img src={slot.modelIcon} alt="" className={styles.summaryRowIcon} />}
                              <span className={styles.summaryRowModel}>{slot.modelLabel}</span>
                              <span className={styles.summaryRowAccount}>
                                {slot.accountLabel}
                                {slot.accountSubLabel && ` · ${slot.accountSubLabel}`}
                              </span>
                            </div>
                            {i < autoWizard.reasoningSlots.length - 1 && (
                              <div className={styles.summaryConnector}>
                                <div className={styles.summaryConnectorLine} />
                                <span className={styles.summaryConnectorLabel}>
                                  ↓ {t("combos.failover_to_next")}
                                </span>
                              </div>
                            )}
                          </div>
                        ))}
                      </div>
                    )}
                  </div>

                  {/* 3. Fast Summary */}
                  <div className={styles.summaryCategorySection}>
                    <div className={styles.summaryCategoryTitle}>
                      <span className={styles.autoCatIconSvg}><Icon icon={speedIcon} size="1.1em" /></span>
                      <span>{t("combos.auto_step_fast")}</span>
                    </div>
                    {autoWizard.fastSlots.length === 0 ? (
                      <span className={styles.autoCatEmpty}>{t("combos.no_slots_in_category")}</span>
                    ) : (
                      <div className={styles.summaryVerticalList}>
                        {autoWizard.fastSlots.map((slot, i) => (
                          <div key={slot.key} className={styles.summaryItemWrapper}>
                            <div className={styles.summaryRow}>
                              <span className={styles.summaryRowIndex}>{i + 1}</span>
                              {slot.modelIcon && <img src={slot.modelIcon} alt="" className={styles.summaryRowIcon} />}
                              <span className={styles.summaryRowModel}>{slot.modelLabel}</span>
                              <span className={styles.summaryRowAccount}>
                                {slot.accountLabel}
                                {slot.accountSubLabel && ` · ${slot.accountSubLabel}`}
                              </span>
                            </div>
                            {i < autoWizard.fastSlots.length - 1 && (
                              <div className={styles.summaryConnector}>
                                <div className={styles.summaryConnectorLine} />
                                <span className={styles.summaryConnectorLabel}>
                                  ↓ {t("combos.failover_to_next")}
                                </span>
                              </div>
                            )}
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                </div>
              </div>
            )}
          </div>

          {/* Navigation Footer */}
          <div className={styles.wizardNav}>
            <Button
              variant="secondary"
              size="small"
              onClick={() => {
                if (autoWizardStep === 1) setAutoWizardOpen(false);
                else setAutoWizardStep((s) => (s - 1) as 1 | 2 | 3 | 4);
              }}
            >
              {autoWizardStep === 1 ? t("common.cancel") : `← ${t("combos.back")}`}
            </Button>
            {autoWizardStep < 4 ? (
              <Button
                variant="primary"
                size="small"
                onClick={() => setAutoWizardStep((s) => (s + 1) as 1 | 2 | 3 | 4)}
              >
                {t("combos.next")} →
              </Button>
            ) : (
              <Button
                variant="primary"
                size="small"
                disabled={autoSaving}
                onClick={() => void saveAutoWizard()}
              >
                {autoSaving ? t("common.processing") : t("combos.save_combo")} ✓
              </Button>
            )}
          </div>
        </div>
      </Modal>

      {/* ── Modal 2: Custom Combo Wizard ── */}
      <Modal
        open={comboWizardOpen}
        title={editingComboId ? t("combos.edit_model_chain") : t("combos.new_model_chain_fallback_combo")}
        busy={comboSaving}
        onClose={() => setComboWizardOpen(false)}
      >
        <div className={styles.wizardWrap}>
          <div className={styles.wizardProgress}>
            {([1, 2, 3, 4] as WizardStep[]).map((s) => (
              <div
                key={s}
                className={`${styles.wizardDot} ${s <= comboStep ? styles.wizardDotActive : ""}`}
                title={stepLabels[s]}
              />
            ))}
            <span className={styles.wizardStepLabel}>{stepLabels[comboStep]}</span>
          </div>

          <div className={styles.wizardScroll}>
            {/* Step 1: Select Accounts */}
            {comboStep === 1 && (
              <div className={styles.stepRoot}>
                <p className={styles.stepHint}>{t("combos.step1_hint")}</p>
                {accountGroups.length === 0 && (
                  <div className={styles.emptySlots}>{t("combos.no_accounts")}</div>
                )}
                {accountGroups.map((group) => (
                  <div key={group.label} className={styles.accountGroup}>
                    <div className={styles.accountGroupHeader}>
                      {group.icon && <img src={group.icon} alt="" className={styles.groupIcon} />}
                      <span>{group.label}</span>
                      <span className={styles.groupCount}>{group.accounts.length}</span>
                    </div>
                    {group.accounts.map((acc) => {
                      const sel = comboWizard.selectedAccountIds.has(acc.id);
                      return (
                        <label key={acc.id} className={`${styles.accountRow} ${sel ? styles.accountRowSel : ""}`}>
                          <input type="checkbox" checked={sel} onChange={() => toggleAccount(acc.id)} />
                          <div className={styles.accountInfo}>
                            <span className={styles.accountLabel}>{acc.label}</span>
                            <span className={styles.accountSub}>{acc.subLabel}</span>
                          </div>
                          <span className={`${styles.accountStatus} ${statusClass(acc.status)}`}>
                            {statusDot(acc.status)}
                          </span>
                        </label>
                      );
                    })}
                  </div>
                ))}
              </div>
            )}

            {/* Step 2: Select Models */}
            {comboStep === 2 && (
              <div className={styles.stepRoot}>
                <p className={styles.stepHint}>{t("combos.step2_hint")}</p>
                <div className={styles.searchBar}>
                  <input
                    type="text"
                    className={styles.searchInput}
                    placeholder={t("combos.search_models_or_accounts")}
                    value={comboSearch}
                    onChange={(e) => setComboSearch(e.target.value)}
                    autoFocus
                  />
                  {comboSearch && (
                    <button
                      className={styles.searchClearBtn}
                      onClick={() => setComboSearch("")}
                      type="button"
                      title={t("common.clear")}
                    >
                      ✕
                    </button>
                  )}
                </div>

                {filteredModelsByAccount.length === 0 && (
                  <div className={styles.emptySlots}>
                    {comboSearch ? t("combos.no_models_found") : t("combos.no_models_for_accounts")}
                  </div>
                )}

                {filteredModelsByAccount.map(({ account, models: mods }) => (
                  <div key={account.id} className={styles.accountGroup}>
                    <div className={styles.accountGroupHeader}>
                      {account.icon && <img src={account.icon} alt="" className={styles.groupIcon} />}
                      <div className={styles.accountHeaderInfo}>
                        <span>{account.label}</span>
                        <span className={styles.accountHeaderSub}>{account.subLabel}</span>
                      </div>
                      <span className={`${styles.accountStatus} ${statusClass(account.status)}`}>
                        {statusDot(account.status)}
                      </span>
                    </div>
                    {mods.map((m) => {
                      const selIdx = comboWizard.selectedSlots.findIndex((s) => s.key === `${account.id}::${m.id}`);
                      const sel = selIdx >= 0;
                      return (
                        <label key={m.id} className={`${styles.modelRow} ${sel ? styles.modelRowSel : ""}`}>
                          <input
                            type="checkbox"
                            checked={sel}
                            onChange={() => toggleComboSlot(account, m)}
                          />
                          <span className={styles.modelLabel}>{m.label}</span>
                          {sel && (
                            <span className={styles.selectedBadge}>
                              ✓ {selIdx + 1}.
                            </span>
                          )}
                        </label>
                      );
                    })}
                  </div>
                ))}
                {comboWizard.selectedSlots.length === 1 && (
                  <p className={styles.stepWarning}>{t("combos.select_at_least_2")}</p>
                )}
              </div>
            )}

            {/* Step 3: Order + Strategy */}
            {comboStep === 3 && (
              <div className={styles.stepRoot}>
                <p className={styles.stepHint}>{t("combos.step3_hint")}</p>
                <div className={styles.slotList}>
                  {comboWizard.selectedSlots.map((slot, idx) => (
                    <div key={slot.key} className={styles.slot}>
                      <span className={styles.slotIndex}>{idx + 1}</span>
                      {slot.modelIcon && <img src={slot.modelIcon} alt="" className={styles.slotIcon} />}
                      <div className={styles.slotInfo}>
                        <span className={styles.slotName}>{slot.modelLabel}</span>
                        <span className={styles.slotAccountBadge}>
                          {slot.accountLabel}
                          {slot.accountSubLabel && ` · ${slot.accountSubLabel}`}
                        </span>
                      </div>
                      <div className={styles.slotMoves}>
                        <button
                          className={styles.slotMoveBtn}
                          disabled={idx === 0}
                          onClick={() => moveComboSlot(idx, -1)}
                          title={t("combos.move_up")}
                        >↑</button>
                        <button
                          className={styles.slotMoveBtn}
                          disabled={idx === comboWizard.selectedSlots.length - 1}
                          onClick={() => moveComboSlot(idx, 1)}
                          title={t("combos.move_down")}
                        >↓</button>
                      </div>
                    </div>
                  ))}
                </div>

                <div className={styles.sectionDivider}>
                  <span>{t("combos.chain_behavior")}</span>
                </div>
                <div className={styles.radioGroup}>
                  {(["fallback", "round_robin", "fill_first", "most_quota"] as const).map((s) => (
                    <label key={s} className={`${styles.radioCard} ${comboWizard.chainStrategy === s ? styles.radioCardSel : ""}`}>
                      <input
                        type="radio"
                        name="chain"
                        value={s}
                        checked={comboWizard.chainStrategy === s}
                        onChange={() => setComboWizard((w) => ({ ...w, chainStrategy: s }))}
                      />
                      <div className={styles.radioCardBody}>
                        <span className={styles.radioCardTitle}>{chainStrategyLabel(s)}</span>
                        <span className={styles.radioCardDesc}>
                          {s === "fallback"
                            ? t("combos.chain_failover_hint")
                            : s === "fill_first"
                            ? t("combos.chain_fill_first_hint")
                            : s === "most_quota"
                            ? t("combos.chain_most_quota_hint")
                            : t("combos.chain_round_robin_hint")}
                        </span>
                      </div>
                    </label>
                  ))}
                </div>
              </div>
            )}

            {/* Step 4: Name + Summary */}
            {comboStep === 4 && (
              <div className={styles.stepRoot}>
                <div className={styles.formField}>
                  <label className={styles.formLabel}>{t("combos.chain_combo_name")}</label>
                  <input
                    className={styles.formInput}
                    placeholder={t("combos.e_g_coding_chain")}
                    value={comboWizard.name}
                    onChange={(e) => setComboWizard((w) => ({ ...w, name: e.target.value }))}
                    autoFocus
                  />
                </div>
                <div className={styles.formField}>
                  <label className={styles.formLabel}>{t("combos.description")}</label>
                  <input
                    className={styles.formInput}
                    placeholder={t("combos.e_g_switch_to_gemini_when_claude")}
                    value={comboWizard.description}
                    onChange={(e) => setComboWizard((w) => ({ ...w, description: e.target.value }))}
                  />
                </div>
                <div className={styles.summaryBox}>
                  <div className={styles.summaryHeader}>
                    <span className={styles.summaryTitle}>{t("combos.summary")}</span>
                    <span className={styles.summaryMeta}>
                      {comboWizard.selectedSlots.length} {t("combos.models_label")} · {chainStrategyLabel(comboWizard.chainStrategy)}
                    </span>
                  </div>
                  <div className={styles.summaryVerticalList}>
                    {comboWizard.selectedSlots.map((slot, i) => (
                      <div key={slot.key} className={styles.summaryItemWrapper}>
                        <div className={styles.summaryRow}>
                          <span className={styles.summaryRowIndex}>{i + 1}</span>
                          {slot.modelIcon && <img src={slot.modelIcon} alt="" className={styles.summaryRowIcon} />}
                          <span className={styles.summaryRowModel}>{slot.modelLabel}</span>
                          <span className={styles.summaryRowAccount}>
                            {slot.accountLabel}
                            {slot.accountSubLabel && ` · ${slot.accountSubLabel}`}
                          </span>
                        </div>
                        {i < comboWizard.selectedSlots.length - 1 && (
                          <div className={styles.summaryConnector}>
                            <div className={styles.summaryConnectorLine} />
                            <span className={styles.summaryConnectorLabel}>
                              {comboWizard.chainStrategy === "fallback"
                                ? `↓ ${t("combos.failover_to_next")}`
                                : `↻ ${t("combos.round_robin_next")}`}
                            </span>
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            )}
          </div>

          <div className={styles.wizardNav}>
            <Button
              variant="secondary"
              size="small"
              onClick={() => {
                if (comboStep === 1) setComboWizardOpen(false);
                else setComboStep((s) => (s - 1) as WizardStep);
              }}
            >
              {comboStep === 1 ? t("common.cancel") : `← ${t("combos.back")}`}
            </Button>
            {comboStep < 4 ? (
              <Button
                variant="primary"
                size="small"
                disabled={comboStep === 1 ? comboWizard.selectedAccountIds.size === 0 : comboStep === 2 ? comboWizard.selectedSlots.length < 2 : false}
                onClick={() => setComboStep((s) => (s + 1) as WizardStep)}
              >
                {t("combos.next")} →
              </Button>
            ) : (
              <Button
                variant="primary"
                size="small"
                disabled={comboWizard.name.trim().length === 0 || comboSaving}
                onClick={() => void handleSaveCombo()}
              >
                {comboSaving ? t("common.processing") : t("combos.save_combo")} ✓
              </Button>
            )}
          </div>
        </div>
      </Modal>
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
          <span className={styles.comboBadge}>{chainStrategyLabel(combo.strategy)}</span>
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
                  {group.count} Hesap Havuzu ({group.accounts.map((a) => a.label).join(", ")})
                </span>
              ) : group.accounts[0] ? (
                <span className={styles.slotReadAccount}>
                  {group.accounts[0].label}
                </span>
              ) : null}
            </div>
            {idx < groupedSlots.length - 1 && (
              <span className={styles.slotArrow}>
                {combo.strategy === "round_robin" ? "↻" : combo.strategy === "fill_first" ? "⇥" : "→"}
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
