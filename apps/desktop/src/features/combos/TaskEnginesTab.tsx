import { useState } from "react";
import type { AutoRouterConfig, ComboSlot } from "../../shared/api";
import type { Account, ModelOption } from "./CombosPage";
import { Card } from "../../shared/ui/Card";
import { Button } from "../../shared/ui/Button";
import { Switch } from "../../shared/ui/Switch";
import { Modal } from "../../shared/ui/Modal";
import { Icon } from "../../shared/ui/Icon";
import { gearIcon } from "../../shared/ui/icons";
import { useMessage } from "../../shared/ui/message";
import styles from "./CombosPage.module.scss";

function isVisionCapable(opt: ModelOption): boolean {
  if (opt.images === false) return false;
  const lower = `${opt.id} ${opt.label}`.toLowerCase();
  // Known text-only non-vision models
  if (
    lower.includes("o3-mini") ||
    lower.includes("o1-mini") ||
    lower.includes("o1-preview") ||
    lower.includes("deepseek-coder") ||
    lower.includes("codex-auto-review") ||
    lower.includes("gpt-5.5") ||
    lower.includes("gpt-reserve")
  ) {
    return false;
  }
  if (opt.images === true) return true;
  // Known vision-capable models
  return (
    lower.includes("flash") ||
    lower.includes("pro") ||
    lower.includes("sonnet") ||
    lower.includes("opus") ||
    lower.includes("haiku") ||
    lower.includes("4o") ||
    lower.includes("vision") ||
    lower.includes("gemini") ||
    lower.includes("claude")
  );
}

export function TaskEnginesTab({
  autoConfig,
  onSaveConfig,
  accounts,
  cleanModelName,
  getSlotAccount,
}: {
  autoConfig: AutoRouterConfig;
  onSaveConfig: (config: AutoRouterConfig) => Promise<void>;
  accounts: Account[];
  cleanModelName: (modelId: string, accounts: Account[]) => string;
  getSlotAccount: (slot: ComboSlot, accounts: Account[]) => Account | undefined;
}) {
  const message = useMessage();
  const [modalTarget, setModalTarget] = useState<"vision" | "subagent" | null>(null);
  const [editingSlots, setEditingSlots] = useState<ComboSlot[]>([]);
  const [searchFilter, setSearchFilter] = useState("");

  const openSlotModal = (target: "vision" | "subagent") => {
    setModalTarget(target);
    const existing = target === "vision" ? autoConfig.vision_slots : autoConfig.subagent_slots;
    setEditingSlots([...(existing || [])]);
    setSearchFilter("");
  };

  const closeSlotModal = () => {
    setModalTarget(null);
    setEditingSlots([]);
  };

  const handleSaveModal = async () => {
    if (!modalTarget) return;
    try {
      const updated: AutoRouterConfig = {
        ...autoConfig,
        updated_at_ms: Date.now(),
        ...(modalTarget === "vision" ? { vision_slots: editingSlots } : { subagent_slots: editingSlots }),
      };
      await onSaveConfig(updated);
      closeSlotModal();
      message(t("combos.task_engine_saved"));
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message(msg || t("combos.engine_error_occurred"));
    }
  };

  const toggleVisionAuto = async (enabled: boolean) => {
    try {
      const updated: AutoRouterConfig = {
        ...autoConfig,
        vision_auto: enabled,
        updated_at_ms: Date.now(),
      };
      await onSaveConfig(updated);
      message(t("combos.task_engine_saved"));
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message(msg || t("combos.engine_error_occurred"));
    }
  };

  const toggleSubagentAuto = async (enabled: boolean) => {
    try {
      const updated: AutoRouterConfig = {
        ...autoConfig,
        subagent_auto: enabled,
        updated_at_ms: Date.now(),
      };
      await onSaveConfig(updated);
      message(t("combos.task_engine_saved"));
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message(msg || t("combos.engine_error_occurred"));
    }
  };

  const addSlot = (modelId: string, accountId: string) => {
    setEditingSlots([
      ...editingSlots,
      {
        model_id: modelId,
        account_ids: accountId ? [accountId] : null,
        slot_strategy: "failover",
      },
    ]);
  };

  const removeSlot = (index: number) => {
    const next = [...editingSlots];
    next.splice(index, 1);
    setEditingSlots(next);
  };

  const moveSlot = (index: number, dir: -1 | 1) => {
    const target = index + dir;
    if (target < 0 || target >= editingSlots.length) return;
    const next = [...editingSlots];
    const item = next[index];
    next[index] = next[target];
    next[target] = item;
    setEditingSlots(next);
  };

  return (
    <div>
      {/* 1. Görsel Çözümleyici (Vision Sidecar) */}
      <Card className={styles.engineCard}>
        <div className={styles.engineHeader}>
          <div className={styles.engineTitleBlock}>
            <div className={styles.engineTitleRow}>
              <strong>{t("combos.engine_vision_title")}</strong>
            </div>
            <span className={styles.engineDesc}>{t("combos.engine_vision_desc")}</span>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span style={{ fontSize: 12, color: "var(--vscode-descriptionForeground)" }}>
              {t("combos.engine_auto_select")}
            </span>
            <Switch
              checked={autoConfig.vision_auto}
              label={t("combos.engine_auto_select")}
              onChange={toggleVisionAuto}
            />
          </div>
        </div>

        {!autoConfig.vision_auto && (
          <div className={styles.engineSlotsChain}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 6 }}>
              <span style={{ fontSize: 12, fontWeight: 600, color: "var(--vscode-foreground)" }}>
                {t("combos.vision_modal_title")}
              </span>
              <Button size="small" variant="secondary" onClick={() => openSlotModal("vision")}>
                <span style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
                  <Icon icon={gearIcon} size="1em" />
                  <span>{t("combos.engine_edit_slots")}</span>
                </span>
              </Button>
            </div>

            {(!autoConfig.vision_slots || autoConfig.vision_slots.length === 0) ? (
              <span style={{ fontSize: 12, color: "var(--vscode-descriptionForeground)", padding: 6 }}>
                {t("combos.engine_no_slots")}
              </span>
            ) : (
              autoConfig.vision_slots.map((s, idx) => {
                const name = cleanModelName(s.model_id, accounts);
                const acc = getSlotAccount(s, accounts);
                const icon = acc?.modelOptions.find((m: ModelOption) => m.id === s.model_id)?.icon || acc?.icon;
                return (
                  <div key={idx} className={styles.engineSlotRow}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <span style={{ fontSize: 12, color: "var(--vscode-descriptionForeground)" }}>{idx + 1}.</span>
                      {icon && <img src={icon} alt="" style={{ width: 14, height: 14 }} />}
                      <span style={{ fontSize: 13, color: "var(--vscode-foreground)", fontWeight: 500 }}>{name}</span>
                      {acc && (
                        <span style={{ fontSize: 11, padding: "2px 6px", background: "rgba(255,255,255,0.05)", borderRadius: 3, color: "#8a8f98" }}>
                          {acc.label}
                        </span>
                      )}
                    </div>
                  </div>
                );
              })
            )}
          </div>
        )}
      </Card>

      {/* 2. Alt Ajanlar (Subagents) */}
      <Card className={styles.engineCard}>
        <div className={styles.engineHeader}>
          <div className={styles.engineTitleBlock}>
            <div className={styles.engineTitleRow}>
              <strong>{t("combos.engine_subagent_title")}</strong>
              <span className={styles.engineStatusBadge}>
                {t("combos.engine_subagent_badge_active")}
              </span>
            </div>
            <span className={styles.engineDesc}>
              {t("combos.engine_subagent_desc")}
            </span>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span style={{ fontSize: 12, color: "var(--vscode-descriptionForeground)" }}>
              {t("combos.engine_auto_select")}
            </span>
            <Switch
              checked={autoConfig.subagent_auto}
              label={t("combos.engine_auto_select")}
              onChange={toggleSubagentAuto}
            />
          </div>
        </div>

        {!autoConfig.subagent_auto && (
          <div className={styles.engineSlotsChain}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 6 }}>
              <span style={{ fontSize: 12, fontWeight: 600, color: "var(--vscode-foreground)" }}>
                {t("combos.subagent_modal_title")}
              </span>
              <Button size="small" variant="secondary" onClick={() => openSlotModal("subagent")}>
                <span style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
                  <Icon icon={gearIcon} size="1em" />
                  <span>{t("combos.engine_edit_slots")}</span>
                </span>
              </Button>
            </div>

            {(!autoConfig.subagent_slots || autoConfig.subagent_slots.length === 0) ? (
              <span style={{ fontSize: 12, color: "var(--vscode-descriptionForeground)", padding: 6 }}>
                {t("combos.engine_no_slots")}
              </span>
            ) : (
              autoConfig.subagent_slots.map((s, idx) => {
                const name = cleanModelName(s.model_id, accounts);
                const acc = getSlotAccount(s, accounts);
                const icon = acc?.modelOptions.find((m: ModelOption) => m.id === s.model_id)?.icon || acc?.icon;
                return (
                  <div key={idx} className={styles.engineSlotRow}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <span style={{ fontSize: 12, color: "var(--vscode-descriptionForeground)" }}>{idx + 1}.</span>
                      {icon && <img src={icon} alt="" style={{ width: 14, height: 14 }} />}
                      <span style={{ fontSize: 13, color: "var(--vscode-foreground)", fontWeight: 500 }}>{name}</span>
                      {acc && (
                        <span style={{ fontSize: 11, padding: "2px 6px", background: "rgba(255,255,255,0.05)", borderRadius: 3, color: "#8a8f98" }}>
                          {acc.label}
                        </span>
                      )}
                    </div>
                  </div>
                );
              })
            )}
          </div>
        )}
      </Card>

      {/* 3. Çoklu Model İncelemesi (Multi-Model Review) */}
      <Card className={styles.engineCard}>
        <div className={styles.engineHeader}>
          <div className={styles.engineTitleBlock}>
            <div className={styles.engineTitleRow}>
              <strong>{t("combos.engine_review_title")}</strong>
              <span className={styles.engineStatusBadge}>
                ✓ {t("combos.engine_review_status_active")}
              </span>
            </div>
            <span className={styles.engineDesc}>{t("combos.engine_review_desc")}</span>
          </div>
        </div>
        <div style={{ fontSize: 12, color: "var(--vscode-descriptionForeground)", fontStyle: "italic", background: "rgba(255,255,255,0.02)", padding: "10px 12px", borderRadius: 6, border: "1px solid rgba(255,255,255,0.05)" }}>
          ℹ️ {t("combos.engine_review_note")}
        </div>
      </Card>

      {/* Model Slot Düzenleme Modalı */}
      {modalTarget && (
        <Modal
          open={Boolean(modalTarget)}
          title={modalTarget === "vision" ? t("combos.vision_modal_title") : t("combos.subagent_modal_title")}
          onClose={closeSlotModal}
          onSubmit={handleSaveModal}
          submitLabel={t("common.save")}
          closeLabel={t("common.cancel")}
        >
          <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
            {/* Mevcut Seçili Slotlar */}
            <div style={{ display: "flex", flexDirection: "column", gap: 6, background: "rgba(255,255,255,0.02)", padding: 10, borderRadius: 6, border: "1px solid rgba(255,255,255,0.06)" }}>
              <span style={{ fontSize: 11, fontWeight: 600, textTransform: "uppercase", color: "#8a8f98" }}>
                {t("combos.engine_priority_order")} ({editingSlots.length})
              </span>
              {editingSlots.length === 0 ? (
                <span style={{ fontSize: 12, color: "#666" }}>{t("combos.engine_no_models_added")}</span>
              ) : (
                editingSlots.map((s, idx) => {
                  const name = cleanModelName(s.model_id, accounts);
                  const acc = getSlotAccount(s, accounts);
                  return (
                    <div key={idx} style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "6px 8px", background: "rgba(255,255,255,0.04)", borderRadius: 4 }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                        <span style={{ fontSize: 11, color: "#888" }}>{idx + 1}.</span>
                        <span style={{ fontSize: 13, color: "#fff", fontWeight: 500 }}>{name}</span>
                        {acc && <span style={{ fontSize: 10, padding: "1px 5px", background: "rgba(255,255,255,0.08)", borderRadius: 2 }}>{acc.label}</span>}
                      </div>
                      <div style={{ display: "flex", gap: 4 }}>
                        <Button size="small" variant="secondary" disabled={idx === 0} onClick={() => moveSlot(idx, -1)}>↑</Button>
                        <Button size="small" variant="secondary" disabled={idx === editingSlots.length - 1} onClick={() => moveSlot(idx, 1)}>↓</Button>
                        <Button size="small" variant="secondary" onClick={() => removeSlot(idx)}>×</Button>
                      </div>
                    </div>
                  );
                })
              )}
            </div>

            {/* Arama ve Model Seçim Listesi */}
            <input
              type="text"
              placeholder={t("combos.engine_search_placeholder")}
              value={searchFilter}
              onChange={(e) => setSearchFilter(e.target.value)}
              style={{ width: "100%", padding: "7px 10px", background: "rgba(255,255,255,0.05)", border: "1px solid rgba(255,255,255,0.1)", borderRadius: 5, color: "#fff", fontSize: 12 }}
            />

            <div style={{ maxHeight: "35vh", overflowY: "auto", display: "flex", flexDirection: "column", gap: 10 }}>
              {accounts.map((acc) => {
                const filtered = acc.modelOptions.filter((opt: ModelOption) => {
                  if (modalTarget === "vision" && !isVisionCapable(opt)) return false;
                  if (!searchFilter.trim()) return true;
                  const query = searchFilter.toLowerCase();
                  return opt.label.toLowerCase().includes(query) || acc.label.toLowerCase().includes(query);
                });
                if (filtered.length === 0) return null;

                return (
                  <div key={acc.id} style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                    <span style={{ fontSize: 11, fontWeight: 600, color: "#8a8f98" }}>{acc.label}</span>
                    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 6 }}>
                      {filtered.map((opt: ModelOption) => {
                        const isAdded = editingSlots.some((s) => s.model_id === opt.id && s.account_ids?.[0] === opt.accountId);
                        return (
                          <button
                            key={`${opt.accountId}::${opt.id}`}
                            type="button"
                            disabled={isAdded}
                            onClick={() => addSlot(opt.id, opt.accountId)}
                            style={{
                              display: "flex",
                              alignItems: "center",
                              justifyContent: "space-between",
                              padding: "6px 10px",
                              background: isAdded ? "rgba(94,106,210,0.15)" : "rgba(255,255,255,0.03)",
                              border: isAdded ? "1px solid #5e6ad2" : "1px solid rgba(255,255,255,0.06)",
                              borderRadius: 4,
                              color: isAdded ? "#7075f5" : "#e0e0e0",
                              fontSize: 12,
                              cursor: isAdded ? "default" : "pointer",
                              textAlign: "left",
                            }}
                          >
                            <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{opt.label}</span>
                            <span>{isAdded ? "✓" : "+"}</span>
                          </button>
                        );
                      })}
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        </Modal>
      )}
    </div>
  );
}
