import { useEffect, useMemo, useState } from "react";
import {
  api,
  type AutoRouterConfig,
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
  refreshIcon,
} from "../../shared/ui/icons";
import { useMessage } from "../../shared/ui/message";
import { useAppStore } from "../../shared/store/appStore";
import { useI18n } from "../../i18n/store";
import { AutoRouterWizard } from "./AutoRouterWizard";
import { CustomComboWizard, type WizardState } from "./CustomComboWizard";
import styles from "./CombosPage.module.scss";

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

export interface WizardSlot {
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

export function CombosPage() {
  const message = useMessage();
  useI18n();
  const { plugins } = useAppStore();

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
      message(next ? "Auto Router Etkinleştirildi" : "Auto Router Devre Dışı Bırakıldı");
    } catch (e) {
      message(String(e));
    }
  };

  const accounts: Account[] = useMemo(() => {
    const list: Account[] = [];
    for (const p of plugins) {
      for (const r of p.resources) {
        for (const item of r.resources) {
          const modelOptions: ModelOption[] = (p.models || []).map((m) => ({
            id: m.id,
            label: m.displayName || m.id,
            icon: m.icon || p.icon || null,
            accountId: item.id,
            accountLabel: item.displayName || item.id,
            pluginId: p.id,
            images: m.images,
          }));

          list.push({
            id: item.id,
            label: item.displayName || item.id,
            subLabel: p.name,
            status: item.state?.status === "cooling" ? "cooling" : "ready",
            icon: p.icon || null,
            pluginId: p.id,
            resourceType: r.type,
            providerLabel: p.name,
            modelOptions,
          });
        }
      }
    }
    return list;
  }, [plugins]);

  const cleanModelName = (mid: string): string => {
    const parts = mid.split("/");
    return parts[parts.length - 1] || mid;
  };

  const openNewCombo = () => {
    setEditingComboId(null);
    setActiveComboWizard(freshWizard());
    setComboWizardOpen(true);
  };

  const handleDeleteCombo = async (id: string) => {
    try { await api.deleteRouterCombo(id); await loadAll(); message("Yedekleme zinciri silindi"); }
    catch (e) { message(String(e)); }
  };

  const handleToggleCombo = async (combo: RouterCombo) => {
    try { await api.upsertRouterCombo({ ...combo, enabled: !combo.enabled, updated_at_ms: Date.now() }); await loadAll(); }
    catch (e) { message(String(e)); }
  };

  const pageContent = (
    <div style={{ display: "flex", flexDirection: "column", gap: "20px", padding: "16px 20px 32px", maxWidth: "1400px", margin: "0 auto", boxSizing: "border-box" }}>
      {/* ── BÖLÜM 1: Akıllı Mod (Auto Router) ── */}
      <Card style={{ background: "#17181c", border: "1px solid rgba(255,255,255,0.08)", borderRadius: "8px", padding: "20px" }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", flexWrap: "wrap", gap: "12px", borderBottom: "1px solid rgba(255,255,255,0.06)", paddingBottom: "16px" }}>
          <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
            <span style={{ display: "flex", alignItems: "center", justifyContent: "center", width: "36px", height: "36px", borderRadius: "8px", background: "rgba(39,195,118,0.1)", color: "#27c376" }}>
              <Icon icon={boltIcon} size="1.3em" />
            </span>
            <div>
              <h3 style={{ margin: 0, fontSize: "16px", fontWeight: 600, color: "#f7f8f8" }}>
                Cursor Auto Router (Akıllı Yönlendirici)
              </h3>
              <p style={{ margin: "3px 0 0", fontSize: "12px", color: "rgba(255,255,255,0.5)" }}>
                Cursor veya harici araçta model olarak <code>auto-smart</code> seçildiğinde istek tipine göre en uygun modeli otomatik devreye sokar.
              </p>
            </div>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
            <Button variant="secondary" size="small" onClick={() => setAutoWizardOpen(true)}>
              <Icon icon={gearIcon} size="1.1em" /> Modelleri Yapılandır
            </Button>
            <Switch
              checked={autoConfig.enabled}
              onChange={() => void toggleAutoSmart()}
              label={autoConfig.enabled ? "Aktif (Açık)" : "Kapalı"}
            />
          </div>
        </div>

        {/* 3 Temiz Kategori Kartı: Kodlama, Düşünme, Hızlı */}
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(280px, 1fr))", gap: "12px", marginTop: "16px" }}>
          {/* Kodlama */}
          <div style={{ background: "rgba(0,0,0,0.25)", border: "1px solid rgba(255,255,255,0.05)", borderRadius: "6px", padding: "14px" }}>
            <div style={{ display: "flex", alignItems: "center", gap: "8px", marginBottom: "10px", color: "#60a5fa", fontSize: "13px", fontWeight: 600 }}>
              <Icon icon={codeIcon} size="1.1em" />
              <span>Kodlama Görevleri (Coding)</span>
            </div>
            <div style={{ fontSize: "12px", color: "rgba(255,255,255,0.7)" }}>
              {autoConfig.coding_slots.length === 0 ? (
                <span style={{ color: "rgba(255,255,255,0.4)" }}>Varsayılan Claude 3.5 Sonnet / GPT-4o kullanılır</span>
              ) : (
                autoConfig.coding_slots.map((s, idx) => (
                  <div key={idx} style={{ padding: "4px 0", borderBottom: "1px solid rgba(255,255,255,0.03)" }}>
                    <strong>{idx + 1}.</strong> {cleanModelName(s.model_id)}
                  </div>
                ))
              )}
            </div>
          </div>

          {/* Düşünme */}
          <div style={{ background: "rgba(0,0,0,0.25)", border: "1px solid rgba(255,255,255,0.05)", borderRadius: "6px", padding: "14px" }}>
            <div style={{ display: "flex", alignItems: "center", gap: "8px", marginBottom: "10px", color: "#c084fc", fontSize: "13px", fontWeight: 600 }}>
              <Icon icon={brainIcon} size="1.1em" />
              <span>Derin Akıl Yürütme (Reasoning)</span>
            </div>
            <div style={{ fontSize: "12px", color: "rgba(255,255,255,0.7)" }}>
              {autoConfig.reasoning_slots.length === 0 ? (
                <span style={{ color: "rgba(255,255,255,0.4)" }}>Varsayılan o3-mini / Gemini Flash Thinking kullanılır</span>
              ) : (
                autoConfig.reasoning_slots.map((s, idx) => (
                  <div key={idx} style={{ padding: "4px 0", borderBottom: "1px solid rgba(255,255,255,0.03)" }}>
                    <strong>{idx + 1}.</strong> {cleanModelName(s.model_id)}
                  </div>
                ))
              )}
            </div>
          </div>

          {/* Hızlı */}
          <div style={{ background: "rgba(0,0,0,0.25)", border: "1px solid rgba(255,255,255,0.05)", borderRadius: "6px", padding: "14px" }}>
            <div style={{ display: "flex", alignItems: "center", gap: "8px", marginBottom: "10px", color: "#34d399", fontSize: "13px", fontWeight: 600 }}>
              <Icon icon={speedIcon} size="1.1em" />
              <span>Hızlı Tamamlama & Tab (Fast)</span>
            </div>
            <div style={{ fontSize: "12px", color: "rgba(255,255,255,0.7)" }}>
              {autoConfig.fast_slots.length === 0 ? (
                <span style={{ color: "rgba(255,255,255,0.4)" }}>Varsayılan Gemini 2.5 Flash / Claude Haiku kullanılır</span>
              ) : (
                autoConfig.fast_slots.map((s, idx) => (
                  <div key={idx} style={{ padding: "4px 0", borderBottom: "1px solid rgba(255,255,255,0.03)" }}>
                    <strong>{idx + 1}.</strong> {cleanModelName(s.model_id)}
                  </div>
                ))
              )}
            </div>
          </div>
        </div>
      </Card>

      {/* ── BÖLÜM 2: Özel Yedekleme Zincirleri (Custom Failover Combos) ── */}
      <div style={{ display: "flex", flexDirection: "column", gap: "12px" }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", flexWrap: "wrap", gap: "8px" }}>
          <div>
            <h3 style={{ margin: 0, fontSize: "15px", fontWeight: 600, color: "#f7f8f8" }}>
              Özel Yedekleme Zincirleri (Failover & Round-Robin)
            </h3>
            <p style={{ margin: "2px 0 0", fontSize: "12px", color: "#8a8f98" }}>
              Biri hata verirse ya da kotası biterse sıradaki modelin otomatik devreye girmesi için özel zincirler tanımlayın.
            </p>
          </div>
          <Button variant="primary" size="small" onClick={openNewCombo}>
            + Yeni Yedekleme Zinciri Oluştur
          </Button>
        </div>

        {combos.length === 0 ? (
          <Card style={{ padding: "32px", textAlign: "center", background: "#17181c", border: "1px dashed rgba(255,255,255,0.1)", borderRadius: "8px" }}>
            <p style={{ margin: 0, color: "rgba(255,255,255,0.5)", fontSize: "13px" }}>
              Henüz özel bir yedekleme zinciri oluşturmadınız. İhtiyaç halinde yukarıdaki butondan ekleyebilirsiniz.
            </p>
          </Card>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: "10px" }}>
            {combos.map((combo) => (
              <Card key={combo.combo_id} style={{ background: "#17181c", border: "1px solid rgba(255,255,255,0.08)", borderRadius: "8px", padding: "16px 18px", display: "flex", justifyContent: "space-between", alignItems: "center", flexWrap: "wrap", gap: "12px" }}>
                <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
                  <span style={{ width: "8px", height: "8px", borderRadius: "50%", background: combo.enabled ? "#27c376" : "#6b7280" }} />
                  <div>
                    <h4 style={{ margin: 0, fontSize: "14px", fontWeight: 600, color: "#f7f8f8" }}>{combo.name}</h4>
                    <span style={{ fontSize: "11px", color: "#8a8f98", fontFamily: "monospace" }}>combo:{combo.combo_id}</span>
                  </div>
                </div>

                {/* Zincirdeki Modeller Akışı */}
                <div style={{ display: "flex", alignItems: "center", gap: "6px", flexWrap: "wrap" }}>
                  {(combo.models || []).map((mid, idx) => (
                    <div key={idx} style={{ display: "flex", alignItems: "center", gap: "6px" }}>
                      <span style={{ background: "rgba(255,255,255,0.06)", border: "1px solid rgba(255,255,255,0.08)", padding: "3px 8px", borderRadius: "4px", fontSize: "11px", color: "#e5e7eb" }}>
                        {cleanModelName(mid)}
                      </span>
                      {idx < (combo.models || []).length - 1 && (
                        <Icon icon={arrowRightIcon} size="0.9em" style={{ color: "rgba(255,255,255,0.3)" }} />
                      )}
                    </div>
                  ))}
                </div>

                <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
                  <Switch
                    checked={combo.enabled}
                    onChange={() => void handleToggleCombo(combo)}
                    label={combo.enabled ? "Aktif" : "Pasif"}
                  />
                  <Button variant="danger" size="small" onClick={() => setDeletingCombo(combo)}>
                    Sil
                  </Button>
                </div>
              </Card>
            ))}
          </div>
        )}
      </div>
    </div>
  );

  return (
    <>
      <PageContent
        title="Yönlendirme & Akıllı Modeller"
        sections={[{ key: "combos", estimatedHeight: 600, content: pageContent }]}
      />

      <ConfirmDialog
        open={deletingCombo !== null}
        title="Yedekleme Zincirini Sil"
        cancelLabel="İptal"
        confirmLabel="Sil"
        onCancel={() => setDeletingCombo(null)}
        onConfirm={() => {
          if (deletingCombo) {
            void handleDeleteCombo(deletingCombo.combo_id);
            setDeletingCombo(null);
          }
        }}
      >
        <p>"{deletingCombo?.name}" zincirini silmek istediğinize emin misiniz?</p>
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
          accounts={accounts}
          onComboSaved={async () => {
            await loadAll();
            setComboWizardOpen(false);
          }}
          cleanModelName={cleanModelName}
        />
      )}
    </>
  );
}
