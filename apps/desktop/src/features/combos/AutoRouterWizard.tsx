import { useEffect, useMemo, useState } from "react";
import type { AutoRouterConfig, ComboSlot } from "../../shared/api";
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
  brainIcon,
  checkIcon,
  closeIcon,
  codeIcon,
  speedIcon,
} from "../../shared/ui/icons";
import type { Account, ModelOption, WizardSlot } from "./CombosPage";
import styles from "./CombosPage.module.scss";

interface AutoRouterWizardProps {
  open: boolean;
  onClose: () => void;
  autoConfig: AutoRouterConfig;
  onConfigSaved: (config: AutoRouterConfig) => void;
  accounts: Account[];
  cleanModelName: (modelId: string, accounts: Account[]) => string;
}

export function AutoRouterWizard({
  open,
  onClose,
  autoConfig,
  onConfigSaved,
  accounts,
  cleanModelName,
}: AutoRouterWizardProps) {
  useI18n();
  const message = useMessage();

  const [step, setStep] = useState<1 | 2 | 3 | 4>(1);
  const [saving, setSaving] = useState(false);
  const [search, setSearch] = useState("");

  const mapComboSlotsToWizard = (slots: ComboSlot[]): WizardSlot[] => {
    return slots.map((s) => {
      const aid = s.account_ids?.[0];
      const acc = aid
        ? accounts.find((a) => a.id === aid)
        : accounts.find((a) => a.id === s.model_id || a.modelOptions.some((m) => m.id === s.model_id));
      const mod =
        acc?.modelOptions.find((m) => m.id === s.model_id) ??
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

  const [slotsState, setSlotsState] = useState<{
    codingSlots: WizardSlot[];
    reasoningSlots: WizardSlot[];
    fastSlots: WizardSlot[];
  }>(() => ({
    codingSlots: mapComboSlotsToWizard(autoConfig.coding_slots),
    reasoningSlots: mapComboSlotsToWizard(autoConfig.reasoning_slots),
    fastSlots: mapComboSlotsToWizard(autoConfig.fast_slots),
  }));

  // Re-sync wizard slot state whenever autoConfig changes or modal opens
  useEffect(() => {
    if (open) {
      setSlotsState({
        codingSlots: mapComboSlotsToWizard(autoConfig.coding_slots),
        reasoningSlots: mapComboSlotsToWizard(autoConfig.reasoning_slots),
        fastSlots: mapComboSlotsToWizard(autoConfig.fast_slots),
      });
      setStep(1);
      setSearch("");
    }
  }, [open, autoConfig]); // eslint-disable-line react-hooks/exhaustive-deps

  const autoStepLabels = [
    "",
    t("combos.auto_step_coding"),
    t("combos.auto_step_reasoning"),
    t("combos.auto_step_fast"),
    t("combos.auto_step_summary"),
  ];

  const getStepSlots = (s: 1 | 2 | 3): WizardSlot[] => {
    if (s === 1) return slotsState.codingSlots;
    if (s === 2) return slotsState.reasoningSlots;
    return slotsState.fastSlots;
  };

  const toggleSlot = (s: 1 | 2 | 3, account: Account, model: ModelOption) => {
    const key = `${account.id}::${model.id}`;
    setSlotsState((prev) => {
      const prop = s === 1 ? "codingSlots" : s === 2 ? "reasoningSlots" : "fastSlots";
      const current = prev[prop];
      const exists = current.some((item) => item.key === key);
      const updated = exists
        ? current.filter((item) => item.key !== key)
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
      return { ...prev, [prop]: updated };
    });
  };

  const moveSlot = (s: 1 | 2 | 3, index: number, delta: -1 | 1) => {
    setSlotsState((prev) => {
      const prop = s === 1 ? "codingSlots" : s === 2 ? "reasoningSlots" : "fastSlots";
      const target = index + delta;
      const current = prev[prop];
      if (target < 0 || target >= current.length) return prev;
      const next = [...current];
      [next[index], next[target]] = [next[target], next[index]];
      return { ...prev, [prop]: next };
    });
  };

  const removeSlot = (s: 1 | 2 | 3, index: number) => {
    setSlotsState((prev) => {
      const prop = s === 1 ? "codingSlots" : s === 2 ? "reasoningSlots" : "fastSlots";
      return { ...prev, [prop]: prev[prop].filter((_, i) => i !== index) };
    });
  };

  const filteredAccounts = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return accounts;
    return accounts
      .map((acc) => {
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
  }, [accounts, search]);

  const saveWizard = async () => {
    setSaving(true);
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
        coding_slots: toComboSlots(slotsState.codingSlots),
        reasoning_slots: toComboSlots(slotsState.reasoningSlots),
        fast_slots: toComboSlots(slotsState.fastSlots),
        updated_at_ms: Date.now(),
      };

      await api.saveAutoRouterConfig(newConfig);
      onConfigSaved(newConfig);
      onClose();
      message(t("combos.auto_config_saved"));
    } catch (e) {
      message(String(e));
    } finally {
      setSaving(false);
    }
  };

  function statusClass(status: Account["status"]) {
    if (status === "ready") return styles.statusReady;
    if (status === "cooling") return styles.statusCooling;
    if (status === "disabled") return styles.statusDisabled;
    return styles.statusInvalid;
  }

  return (
    <Modal
      open={open}
      title={t("combos.auto_wizard_title")}
      busy={saving}
      onClose={onClose}
    >
      <div className={styles.wizardWrap}>
        <div className={styles.wizardProgress}>
          {([1, 2, 3, 4] as const).map((s) => (
            <div
              key={s}
              className={`${styles.wizardDot} ${s <= step ? styles.wizardDotActive : ""}`}
              title={autoStepLabels[s]}
            />
          ))}
          <span className={styles.wizardStepLabel}>{autoStepLabels[step]}</span>
        </div>

        <div className={styles.wizardScroll}>
          {(step === 1 || step === 2 || step === 3) && (() => {
            const currentStep = step;
            const slots = getStepSlots(currentStep);
            const info =
              currentStep === 1
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
                : currentStep === 2
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
                <div className={styles.autoStepHeader}>
                  <div className={styles.autoCatHeader}>
                    <span className={styles.autoCatIconSvg}><Icon icon={info.icon} size="1.1em" /></span>
                    <span style={{ fontSize: "0.95rem" }}>{info.title}</span>
                  </div>
                </div>
                <div className={styles.autoInfoBanner}>
                  <p className={styles.autoInfoText}>{info.desc}</p>
                  <div className={styles.autoExampleRow}>
                    <span className={styles.autoExampleLabel}>{t("combos.example_models")}</span>
                    {info.chips.map((chip) => (
                      <span key={chip} className={styles.autoExampleChip}>{chip}</span>
                    ))}
                  </div>
                </div>

                {slots.length > 0 && (
                  <div className={styles.selectedSlotsSection}>
                    <span className={styles.selectedSlotsHeader}>
                      {t("combos.attempt_order")} ({slots.length} {t("combos.models_label")})
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
                              onClick={() => moveSlot(currentStep, idx, -1)}
                              title={t("combos.move_up")}
                            >
                              <Icon icon={arrowUpIcon} size="1em" />
                            </button>
                            <button
                              className={styles.slotMoveBtn}
                              disabled={idx === slots.length - 1}
                              onClick={() => moveSlot(currentStep, idx, 1)}
                              title={t("combos.move_down")}
                            >
                              <Icon icon={arrowDownIcon} size="1em" />
                            </button>
                          </div>
                          <button
                            className={styles.slotRemoveBtn}
                            onClick={() => removeSlot(currentStep, idx)}
                            title={t("combos.remove_from_list")}
                          >
                            <Icon icon={closeIcon} size="1em" />
                          </button>
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                <div className={styles.searchBar}>
                  <input
                    type="text"
                    className={styles.searchInput}
                    placeholder={t("combos.search_models_or_accounts")}
                    value={search}
                    onChange={(e) => setSearch(e.target.value)}
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

                {filteredAccounts.map((acc) => (
                  <div key={acc.id} className={styles.accountGroup}>
                    <div className={styles.accountGroupHeader}>
                      {acc.icon && <img src={acc.icon} alt="" className={styles.groupIcon} />}
                      <div className={styles.accountHeaderInfo}>
                        <span>{acc.label}</span>
                        <span className={styles.accountHeaderSub}>{acc.subLabel}</span>
                      </div>
                      <span className={`${styles.accountStatus} ${statusClass(acc.status)}`}>
                        <span className={styles.statusDot} />
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
                            onChange={() => toggleSlot(currentStep, acc, m)}
                          />
                          <span className={styles.modelLabel}>{m.label}</span>
                          {isSel && (
                            <span className={styles.selectedBadge}>
                              <Icon icon={checkIcon} size="1em" /> {selIdx + 1}.
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

          {step === 4 && (
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
                  {slotsState.codingSlots.length === 0 ? (
                    <span className={styles.autoCatEmpty}>{t("combos.no_slots_in_category")}</span>
                  ) : (
                    <div className={styles.summaryVerticalList}>
                      {slotsState.codingSlots.map((slot, i) => (
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
                          {i < slotsState.codingSlots.length - 1 && (
                            <div className={styles.summaryConnector}>
                              <div className={styles.summaryConnectorLine} />
                              <span className={styles.summaryConnectorLabel} style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                                <Icon icon={arrowDownIcon} size="0.9em" />
                                <span>{t("combos.failover_to_next")}</span>
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
                  {slotsState.reasoningSlots.length === 0 ? (
                    <span className={styles.autoCatEmpty}>{t("combos.no_slots_in_category")}</span>
                  ) : (
                    <div className={styles.summaryVerticalList}>
                      {slotsState.reasoningSlots.map((slot, i) => (
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
                          {i < slotsState.reasoningSlots.length - 1 && (
                            <div className={styles.summaryConnector}>
                              <div className={styles.summaryConnectorLine} />
                              <span className={styles.summaryConnectorLabel} style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                                <Icon icon={arrowDownIcon} size="0.9em" />
                                <span>{t("combos.failover_to_next")}</span>
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
                  {slotsState.fastSlots.length === 0 ? (
                    <span className={styles.autoCatEmpty}>{t("combos.no_slots_in_category")}</span>
                  ) : (
                    <div className={styles.summaryVerticalList}>
                      {slotsState.fastSlots.map((slot, i) => (
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
                          {i < slotsState.fastSlots.length - 1 && (
                            <div className={styles.summaryConnector}>
                              <div className={styles.summaryConnectorLine} />
                              <span className={styles.summaryConnectorLabel} style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                                <Icon icon={arrowDownIcon} size="0.9em" />
                                <span>{t("combos.failover_to_next")}</span>
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

        <div className={styles.wizardNav}>
          <Button
            variant="secondary"
            size="small"
            onClick={() => {
              if (step === 1) onClose();
              else setStep((s) => (s - 1) as 1 | 2 | 3 | 4);
            }}
          >
            {step === 1 ? t("common.cancel") : <><Icon icon={arrowLeftIcon} size="0.9em" /> {t("combos.back")}</>}
          </Button>
          {step < 4 ? (
            <Button
              variant="primary"
              size="small"
              onClick={() => setStep((s) => (s + 1) as 1 | 2 | 3 | 4)}
            >
              {t("combos.next")} <Icon icon={arrowRightIcon} size="0.9em" />
            </Button>
          ) : (
            <Button
              variant="primary"
              size="small"
              disabled={saving}
              onClick={() => void saveWizard()}
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
