import { useEffect, useMemo, useState } from "react";
import type { ComboSlot, RouterCombo } from "../../shared/api";
import { api } from "../../shared/api";
import { useI18n } from "../../i18n/store";
import { useMessage } from "../../shared/ui/message";
import { Button } from "../../shared/ui/Button";
import { Modal } from "../../shared/ui/Modal";
import { Icon } from "../../shared/ui/Icon";
import {
  arrowDownIcon,
  arrowLeftIcon,
  arrowRightIcon,
  arrowUpIcon,
  checkIcon,
  closeIcon,
  refreshIcon,
} from "../../shared/ui/icons";
import type { Account, ModelOption, WizardSlot } from "./CombosPage";
import styles from "./CombosPage.module.scss";

type WizardStep = 1 | 2 | 3 | 4;

export interface WizardState {
  combo_id: string;
  selectedAccountIds: Set<string>;
  selectedSlots: WizardSlot[];
  chainStrategy: "fallback" | "round_robin" | "fill_first" | "most_quota";
  name: string;
  description: string;
}

interface CustomComboWizardProps {
  open: boolean;
  onClose: () => void;
  editingComboId: string | null;
  initialWizard: WizardState;
  combos: RouterCombo[];
  onComboSaved: () => Promise<void>;
  accounts: Account[];
  accountGroups: { label: string; icon: string | null; accounts: Account[] }[];
}

export function CustomComboWizard({
  open,
  onClose,
  editingComboId,
  initialWizard,
  combos,
  onComboSaved,
  accounts,
  accountGroups,
}: CustomComboWizardProps) {
  const { t } = useI18n();
  const message = useMessage();

  const [step, setStep] = useState<WizardStep>(1);
  const [saving, setSaving] = useState(false);
  const [search, setSearch] = useState("");
  const [wizard, setWizard] = useState<WizardState>(initialWizard);

  // Sync state whenever modal opens or initialWizard changes
  useEffect(() => {
    if (open) {
      setWizard(initialWizard);
      setStep(1);
      setSearch("");
    }
  }, [open, initialWizard]);

  const stepLabels = [
    "",
    t("combos.step_accounts"),
    t("combos.step_models"),
    t("combos.step_order"),
    t("combos.step_summary"),
  ];

  function statusClass(status: Account["status"]) {
    if (status === "ready") return styles.statusReady;
    if (status === "cooling") return styles.statusCooling;
    if (status === "disabled") return styles.statusDisabled;
    return styles.statusInvalid;
  }

  function chainStrategyLabel(s: string) {
    if (s === "round_robin") return t("combos.chain_round_robin");
    if (s === "fill_first") return t("combos.chain_fill_first");
    if (s === "most_quota") return t("combos.chain_most_quota");
    return t("combos.chain_failover");
  }

  const modelsByAccount = useMemo(() => {
    return accounts
      .filter((a) => wizard.selectedAccountIds.has(a.id))
      .map((a) => ({ account: a, models: a.modelOptions }))
      .filter((g) => g.models.length > 0);
  }, [accounts, wizard.selectedAccountIds]);

  const filteredModelsByAccount = useMemo(() => {
    const q = search.trim().toLowerCase();
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
  }, [modelsByAccount, search]);

  const toggleAccount = (id: string) =>
    setWizard((w) => {
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
    setWizard((w) => {
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
    setWizard((w) => {
      const target = index + delta;
      if (target < 0 || target >= w.selectedSlots.length) return w;
      const next = [...w.selectedSlots];
      [next[index], next[target]] = [next[target], next[index]];
      return { ...w, selectedSlots: next };
    });
  };

  const handleSave = async () => {
    if (!wizard.name.trim()) {
      message(t("combos.please_enter_a_name_for_the_comb"));
      return;
    }
    setSaving(true);
    try {
      const existing = combos.find((c) => c.combo_id === wizard.combo_id);
      const slots: ComboSlot[] = wizard.selectedSlots.map((s) => ({
        model_id: s.modelId,
        account_ids: s.pluginId ? [s.accountId] : null,
        slot_strategy: wizard.chainStrategy,
      }));

      await api.upsertRouterCombo({
        combo_id: wizard.combo_id,
        name: wizard.name.trim(),
        description: wizard.description.trim() || null,
        strategy: wizard.chainStrategy,
        models: slots.map((s) => s.model_id),
        slots,
        enabled: existing?.enabled ?? true,
        created_at_ms: existing?.created_at_ms ?? Date.now(),
        updated_at_ms: Date.now(),
      });

      onClose();
      await onComboSaved();
      message(editingComboId ? t("combos.combo_updated") : t("combos.smart_combo_created_successfully"));
    } catch (e) {
      message(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      open={open}
      title={editingComboId ? t("combos.edit_model_chain") : t("combos.new_model_chain_fallback_combo")}
      busy={saving}
      onClose={onClose}
    >
      <div className={styles.wizardWrap}>
        <div className={styles.wizardProgress}>
          {([1, 2, 3, 4] as WizardStep[]).map((s) => (
            <div
              key={s}
              className={`${styles.wizardDot} ${s <= step ? styles.wizardDotActive : ""}`}
              title={stepLabels[s]}
            />
          ))}
          <span className={styles.wizardStepLabel}>{stepLabels[step]}</span>
        </div>

        <div className={styles.wizardScroll}>
          {/* Step 1: Select Accounts */}
          {step === 1 && (
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
                    const sel = wizard.selectedAccountIds.has(acc.id);
                    return (
                      <label key={acc.id} className={`${styles.accountRow} ${sel ? styles.accountRowSel : ""}`}>
                        <input type="checkbox" checked={sel} onChange={() => toggleAccount(acc.id)} />
                        <div className={styles.accountInfo}>
                          <span className={styles.accountLabel}>{acc.label}</span>
                          <span className={styles.accountSub}>{acc.subLabel}</span>
                        </div>
                        <span className={`${styles.accountStatus} ${statusClass(acc.status)}`}>
                          <span className={styles.statusDot} />
                        </span>
                      </label>
                    );
                  })}
                </div>
              ))}
            </div>
          )}

          {/* Step 2: Select Models */}
          {step === 2 && (
            <div className={styles.stepRoot}>
              <p className={styles.stepHint}>{t("combos.step2_hint")}</p>
              <div className={styles.searchBar}>
                <input
                  type="text"
                  className={styles.searchInput}
                  placeholder={t("combos.search_models_or_accounts")}
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  autoFocus
                />
                {search && (
                  <button
                    className={styles.searchClearBtn}
                    onClick={() => setSearch("")}
                    type="button"
                    title={t("common.clear")}
                  >
                    <Icon icon={closeIcon} size="1em" />
                  </button>
                )}
              </div>

              {filteredModelsByAccount.length === 0 && (
                <div className={styles.emptySlots}>
                  {search ? t("combos.no_models_found") : t("combos.no_models_for_accounts")}
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
                      <span className={styles.statusDot} />
                    </span>
                  </div>
                  {mods.map((m) => {
                    const selIdx = wizard.selectedSlots.findIndex((s) => s.key === `${account.id}::${m.id}`);
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
                            <Icon icon={checkIcon} size="1em" /> {selIdx + 1}.
                          </span>
                        )}
                      </label>
                    );
                  })}
                </div>
              ))}
              {wizard.selectedSlots.length === 1 && (
                <p className={styles.stepWarning}>{t("combos.select_at_least_2")}</p>
              )}
            </div>
          )}

          {/* Step 3: Order + Strategy */}
          {step === 3 && (
            <div className={styles.stepRoot}>
              <p className={styles.stepHint}>{t("combos.step3_hint")}</p>
              <div className={styles.slotList}>
                {wizard.selectedSlots.map((slot, idx) => (
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
                      >
                        <Icon icon={arrowUpIcon} size="1em" />
                      </button>
                      <button
                        className={styles.slotMoveBtn}
                        disabled={idx === wizard.selectedSlots.length - 1}
                        onClick={() => moveComboSlot(idx, 1)}
                        title={t("combos.move_down")}
                      >
                        <Icon icon={arrowDownIcon} size="1em" />
                      </button>
                    </div>
                  </div>
                ))}
              </div>

              <div className={styles.sectionDivider}>
                <span>{t("combos.chain_behavior")}</span>
              </div>
              <div className={styles.radioGroup}>
                {(["fallback", "round_robin", "fill_first", "most_quota"] as const).map((s) => (
                  <label key={s} className={`${styles.radioCard} ${wizard.chainStrategy === s ? styles.radioCardSel : ""}`}>
                    <input
                      type="radio"
                      name="chain"
                      value={s}
                      checked={wizard.chainStrategy === s}
                      onChange={() => setWizard((w) => ({ ...w, chainStrategy: s }))}
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
          {step === 4 && (
            <div className={styles.stepRoot}>
              <div className={styles.formField}>
                <label className={styles.formLabel}>{t("combos.chain_combo_name")}</label>
                <input
                  className={styles.formInput}
                  placeholder={t("combos.e_g_coding_chain")}
                  value={wizard.name}
                  onChange={(e) => setWizard((w) => ({ ...w, name: e.target.value }))}
                  autoFocus
                />
              </div>
              <div className={styles.formField}>
                <label className={styles.formLabel}>{t("combos.description")}</label>
                <input
                  className={styles.formInput}
                  placeholder={t("combos.e_g_switch_to_gemini_when_claude")}
                  value={wizard.description}
                  onChange={(e) => setWizard((w) => ({ ...w, description: e.target.value }))}
                />
              </div>
              <div className={styles.summaryBox}>
                <div className={styles.summaryHeader}>
                  <span className={styles.summaryTitle}>{t("combos.summary")}</span>
                  <span className={styles.summaryMeta}>
                    {wizard.selectedSlots.length} {t("combos.models_label")} · {chainStrategyLabel(wizard.chainStrategy)}
                  </span>
                </div>
                <div className={styles.summaryVerticalList}>
                  {wizard.selectedSlots.map((slot, i) => (
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
                      {i < wizard.selectedSlots.length - 1 && (
                        <div className={styles.summaryConnector}>
                          <div className={styles.summaryConnectorLine} />
                          <span className={styles.summaryConnectorLabel} style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                            <Icon icon={wizard.chainStrategy === "fallback" ? arrowDownIcon : refreshIcon} size="0.9em" />
                            <span>{wizard.chainStrategy === "fallback" ? t("combos.failover_to_next") : t("combos.round_robin_next")}</span>
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

        {/* Navigation Footer */}
        <div className={styles.wizardNav}>
          <Button
            variant="secondary"
            size="small"
            onClick={() => {
              if (step === 1) onClose();
              else setStep((s) => (s - 1) as WizardStep);
            }}
          >
            {step === 1 ? t("common.cancel") : <><Icon icon={arrowLeftIcon} size="0.9em" /> {t("combos.back")}</>}
          </Button>
          {step < 4 ? (
            <Button
              variant="primary"
              size="small"
              disabled={step === 1 ? wizard.selectedAccountIds.size === 0 : step === 2 ? wizard.selectedSlots.length < 2 : false}
              onClick={() => setStep((s) => (s + 1) as WizardStep)}
            >
              {t("combos.next")} <Icon icon={arrowRightIcon} size="0.9em" />
            </Button>
          ) : (
            <Button
              variant="primary"
              size="small"
              disabled={saving}
              onClick={() => void handleSave()}
            >
              <span style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
                <span>{saving ? t("common.processing") : t("combos.save_combo")}</span>
                {!saving && <Icon icon={checkIcon} size="1em" />}
              </span>
            </Button>
          )}
        </div>
      </div>
    </Modal>
  );
}
