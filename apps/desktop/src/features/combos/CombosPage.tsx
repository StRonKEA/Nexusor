import { useEffect, useMemo, useState } from "react";
import { api, configuredPluginModels, type AutoRouterConfig, type RouterCombo } from "../../shared/api";
import { PageContent } from "../../shell/layout/PageContent";
import { Card } from "../../shared/ui/Card";
import { Button } from "../../shared/ui/Button";
import { ConfirmDialog } from "../../shared/ui/ConfirmDialog";
import { Switch } from "../../shared/ui/Switch";
import { Icon } from "../../shared/ui/Icon";
import { boltIcon, arrowRightIcon } from "../../shared/ui/icons";
import { useMessage } from "../../shared/ui/message";
import { useAppStore } from "../../shared/store/appStore";
import styles from "./CombosPage.module.scss";

export function CombosPage() {
  const message = useMessage();
  const { plugins, models } = useAppStore();

  const [combos, setCombos] = useState<RouterCombo[]>([]);
  const [deletingCombo, setDeletingCombo] = useState<RouterCombo | null>(null);

  // Auto Router state
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

  // New combo inline creation draft
  const [showCreateCombo, setShowCreateCombo] = useState(false);
  const [comboName, setComboName] = useState("");
  const [comboStrategy, setComboStrategy] = useState<"fallback" | "round_robin">("fallback");
  const [selectedModelList, setSelectedModelList] = useState<string[]>([]);

  // Collect all usable model IDs across plugins and manual configs
  const allAvailableModels = useMemo(() => {
    const list: Array<{ id: string; name: string }> = [];
    const pModels = configuredPluginModels(plugins);
    for (const m of pModels) {
      list.push({ id: m.id, name: `${m.displayName || m.id}` });
    }
    for (const m of models) {
      if (!list.some((item) => item.id === m.model_id)) {
        list.push({ id: m.model_id, name: `${m.display_name || m.model_id} (Manuel)` });
      }
    }
    return list;
  }, [plugins, models]);

  const loadData = async () => {
    try {
      setCombos(await api.routerCombos());
    } catch (e) {
      message(String(e));
    }
    try {
      setAutoConfig(await api.autoRouterConfig());
    } catch {
      // default
    }
  };

  useEffect(() => {
    void loadData();
  }, [plugins.length]); // eslint-disable-line react-hooks/exhaustive-deps

  const toggleAutoRouter = async (enabled: boolean) => {
    try {
      const updated = { ...autoConfig, enabled, updated_at_ms: Date.now() };
      await api.saveAutoRouterConfig(updated);
      setAutoConfig(updated);
      await api.setModelCustomization("auto-smart", { custom_name: null, enabled });
      message(enabled ? "Auto Router Etkinleştirildi" : "Auto Router Kapatıldı");
    } catch (e) {
      message(String(e));
    }
  };

  const updateAutoPrimaryModel = async (
    category: "coding" | "reasoning" | "fast",
    modelId: string
  ) => {
    try {
      const slot = modelId ? [{ model_id: modelId, account_ids: null, slot_strategy: "fallback" }] : [];
      const updated = {
        ...autoConfig,
        [`${category}_slots`]: slot,
        updated_at_ms: Date.now(),
      };
      await api.saveAutoRouterConfig(updated);
      setAutoConfig(updated);
      message("Model seçimi güncellendi");
    } catch (e) {
      message(String(e));
    }
  };

  const toggleModelInNewCombo = (id: string) => {
    setSelectedModelList((prev) =>
      prev.includes(id) ? prev.filter((m) => m !== id) : [...prev, id]
    );
  };

  const saveNewCombo = async () => {
    if (!comboName.trim()) {
      message("Lütfen zincir için bir isim girin");
      return;
    }
    if (selectedModelList.length < 2) {
      message("Bir yedekleme zinciri için en az 2 model seçmelisiniz");
      return;
    }

    try {
      const now = Date.now();
      const newCombo: RouterCombo = {
        combo_id: `combo-${now}`,
        name: comboName.trim(),
        description: "",
        strategy: comboStrategy,
        enabled: true,
        models: selectedModelList,
        slots: selectedModelList.map((m) => ({
          model_id: m,
          account_ids: null,
          slot_strategy: comboStrategy,
        })),
        created_at_ms: now,
        updated_at_ms: now,
      };

      await api.upsertRouterCombo(newCombo);
      await loadData();
      setShowCreateCombo(false);
      setComboName("");
      setSelectedModelList([]);
      message("Yedekleme zinciri başarıyla kaydedildi");
    } catch (e) {
      message(String(e));
    }
  };

  const toggleComboState = async (combo: RouterCombo) => {
    try {
      await api.upsertRouterCombo({
        ...combo,
        enabled: !combo.enabled,
        updated_at_ms: Date.now(),
      });
      await loadData();
    } catch (e) {
      message(String(e));
    }
  };

  const deleteCombo = async (id: string) => {
    try {
      await api.deleteRouterCombo(id);
      await loadData();
      message("Yedekleme zinciri silindi");
    } catch (e) {
      message(String(e));
    }
  };

  const cleanName = (id: string) => {
    const parts = id.split("/");
    return parts[parts.length - 1] || id;
  };

  const content = (
    <div className={styles.page}>
      {/* ── BÖLÜM 1: Cursor Auto Router (Akıllı Yönlendirici) ── */}
      <Card className={styles.card}>
        <div className={styles.cardHeader}>
          <div className={styles.titleGroup}>
            <span className={styles.iconBadge}>
              <Icon icon={boltIcon} size="1.2em" />
            </span>
            <div>
              <h3>Auto Router (Akıllı Yönlendirici)</h3>
              <p>Model olarak <code>auto-smart</code> seçildiğinde istek tipine göre modeli otomatik belirler.</p>
            </div>
          </div>
          <Switch
            checked={autoConfig.enabled}
            label={autoConfig.enabled ? "Auto Mod: Açık" : "Auto Mod: Kapalı"}
            onChange={toggleAutoRouter}
          />
        </div>

        <div className={styles.autoGrid}>
          {/* 1. Kodlama */}
          <div className={styles.autoCategory}>
            <span className={styles.catTitle}>💻 Kodlama Görevleri (Coding)</span>
            <span className={styles.catDesc}>Kod yazma, düzenleme ve dosya analizleri</span>
            <select
              className={styles.selectBox}
              value={autoConfig.coding_slots[0]?.model_id || ""}
              onChange={(e) => updateAutoPrimaryModel("coding", e.target.value)}
            >
              <option value="">(Otomatik - En Uygun Modeli Seç)</option>
              {allAvailableModels.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            </select>
          </div>

          {/* 2. Düşünme */}
          <div className={styles.autoCategory}>
            <span className={styles.catTitle}>🧠 Derin Akıl Yürütme (Reasoning)</span>
            <span className={styles.catDesc}>Karmaşık mimari planlar ve matematik</span>
            <select
              className={styles.selectBox}
              value={autoConfig.reasoning_slots[0]?.model_id || ""}
              onChange={(e) => updateAutoPrimaryModel("reasoning", e.target.value)}
            >
              <option value="">(Otomatik - Düşünme Modelini Seç)</option>
              {allAvailableModels.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            </select>
          </div>

          {/* 3. Hızlı */}
          <div className={styles.autoCategory}>
            <span className={styles.catTitle}>⚡ Hızlı Tamamlama & Tab (Fast)</span>
            <span className={styles.catDesc}>Satır içi tamamlama ve kısa sorular</span>
            <select
              className={styles.selectBox}
              value={autoConfig.fast_slots[0]?.model_id || ""}
              onChange={(e) => updateAutoPrimaryModel("fast", e.target.value)}
            >
              <option value="">(Otomatik - En Hızlı Modeli Seç)</option>
              {allAvailableModels.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            </select>
          </div>
        </div>
      </Card>

      {/* ── BÖLÜM 2: Özel Yedekleme Zincirleri (Failover & Round-Robin) ── */}
      <div style={{ display: "flex", flexDirection: "column", gap: "12px" }}>
        <div className={styles.sectionHeading}>
          <div>
            <h3>Özel Yedekleme Zincirleri (Failover)</h3>
            <p>Biri hata verirse veya kotası biterse sıradaki modelin devreye girmesi için sıralı zincirler.</p>
          </div>
          <Button
            variant={showCreateCombo ? "secondary" : "primary"}
            size="small"
            onClick={() => setShowCreateCombo((prev) => !prev)}
          >
            {showCreateCombo ? "İptal Et" : "+ Yeni Zincir Oluştur"}
          </Button>
        </div>

        {/* Yeni Zincir Oluşturma Kutusu (Açılır-Kapanır Basit Form) */}
        {showCreateCombo && (
          <div className={styles.createCard}>
            <div className={styles.createRow}>
              <input
                type="text"
                placeholder="Zincir Adı (Örn: Claude -> Gemini Yedekleme)"
                value={comboName}
                onChange={(e) => setComboName(e.target.value)}
              />
              <select
                value={comboStrategy}
                onChange={(e) => setComboStrategy(e.target.value as "fallback" | "round_robin")}
              >
                <option value="fallback">Failover (Sırayla Yedekle)</option>
                <option value="round_robin">Round-Robin (Dönüşümlü Yük Dağıt)</option>
              </select>
            </div>

            <div className={styles.slotPicker}>
              <span className={styles.slotPickerLabel}>
                Zincire Eklenecek Modelleri Sırayla Seçin (Tıklayarak ekleyin):
              </span>
              <div className={styles.slotChips}>
                {allAvailableModels.map((m) => {
                  const isSelected = selectedModelList.includes(m.id);
                  const orderIdx = selectedModelList.indexOf(m.id);
                  return (
                    <button
                      key={m.id}
                      type="button"
                      className={`${styles.slotChip} ${isSelected ? styles.slotChipSelected : ""}`}
                      onClick={() => toggleModelInNewCombo(m.id)}
                    >
                      {isSelected && <span>{orderIdx + 1}.</span>}
                      <span>{cleanName(m.id)}</span>
                    </button>
                  );
                })}
              </div>
            </div>

            <div style={{ display: "flex", justifyContent: "flex-end", gap: "8px" }}>
              <Button size="small" variant="secondary" onClick={() => setShowCreateCombo(false)}>
                Vazgeç
              </Button>
              <Button size="small" variant="primary" onClick={saveNewCombo}>
                Zinciri Kaydet
              </Button>
            </div>
          </div>
        )}

        {/* Mevcut Zincirler Listesi */}
        {combos.length === 0 ? (
          <Card style={{ padding: "30px", textAlign: "center", background: "#17181c", border: "1px dashed rgba(255,255,255,0.08)", borderRadius: "8px" }}>
            <p style={{ margin: 0, color: "rgba(255,255,255,0.45)", fontSize: "13px" }}>
              Henüz özel bir yedekleme zinciri oluşturulmadı.
            </p>
          </Card>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: "10px" }}>
            {combos.map((combo) => (
              <div key={combo.combo_id} className={styles.comboItem}>
                <div className={styles.comboLeft}>
                  <div className={styles.dot} data-active={combo.enabled ? "true" : "false"} />
                  <div>
                    <h4 className={styles.comboName}>{combo.name}</h4>
                    <span className={styles.comboId}>combo:{combo.combo_id}</span>
                  </div>
                </div>

                <div className={styles.chainFlow}>
                  {(combo.models || []).map((mid, idx) => (
                    <div key={idx} style={{ display: "flex", alignItems: "center", gap: "6px" }}>
                      <span className={styles.chainNode}>{cleanName(mid)}</span>
                      {idx < (combo.models || []).length - 1 && (
                        <Icon icon={arrowRightIcon} size="0.9em" style={{ color: "rgba(255,255,255,0.25)" }} />
                      )}
                    </div>
                  ))}
                </div>

                <div className={styles.comboActions}>
                  <Switch
                    checked={combo.enabled}
                    label={combo.enabled ? "Açık" : "Kapalı"}
                    onChange={() => void toggleComboState(combo)}
                  />
                  <Button variant="secondary" size="small" onClick={() => setDeletingCombo(combo)}>
                    Sil
                  </Button>
                </div>
              </div>
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
        sections={[{ key: "combos", estimatedHeight: 600, content }]}
      />

      <ConfirmDialog
        open={deletingCombo !== null}
        title="Zinciri Sil"
        cancelLabel="Vazgeç"
        confirmLabel="Sil"
        onCancel={() => setDeletingCombo(null)}
        onConfirm={() => {
          if (deletingCombo) {
            void deleteCombo(deletingCombo.combo_id);
            setDeletingCombo(null);
          }
        }}
      >
        <p>"{deletingCombo?.name}" yedekleme zincirini silmek istediğinize emin misiniz?</p>
      </ConfirmDialog>
    </>
  );
}
