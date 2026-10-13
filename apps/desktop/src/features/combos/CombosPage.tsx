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

  const [showCreateCombo, setShowCreateCombo] = useState(false);
  const [comboName, setComboName] = useState("");
  const [comboStrategy, setComboStrategy] = useState<"fallback" | "round_robin">("fallback");
  const [selectedModelList, setSelectedModelList] = useState<string[]>([]);
  const [activeAccountFilter, setActiveAccountFilter] = useState<string>("all");

  // Group models logically by their accounts/providers
  const { accountsList, modelsByAccount } = useMemo(() => {
    const accMap = new Map<string, { id: string; name: string; type: string }>();
    const modMap = new Map<string, Array<{ id: string; name: string; modelId: string }>>();

    // 1. Plugin Accounts & Models
    const pModels = configuredPluginModels(plugins);
    for (const pm of pModels) {
      const accKey = pm.pluginId;
      if (!accMap.has(accKey)) {
        accMap.set(accKey, { id: accKey, name: pm.pluginName, type: "Hesap" });
      }
      const existing = modMap.get(accKey) || [];
      existing.push({ id: pm.id, name: pm.displayName || pm.modelId, modelId: pm.modelId });
      modMap.set(accKey, existing);
    }

    // 2. Manual Models
    if (models.length > 0) {
      const manualKey = "custom-manual";
      accMap.set(manualKey, { id: manualKey, name: "Özel API Modelleri", type: "Manuel" });
      const manModels = models.map((m) => ({
        id: m.model_id,
        name: m.display_name || m.model_id,
        modelId: m.model_id,
      }));
      modMap.set(manualKey, manModels);
    }

    return {
      accountsList: Array.from(accMap.values()),
      modelsByAccount: modMap,
    };
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
      {/* ── BÖLÜM 1: Çekirdek Yönlendirme Çalışma Mantığı Bilgi Kartı ── */}
      <Card className={styles.card}>
        <div className={styles.cardHeader}>
          <div className={styles.titleGroup}>
            <span className={styles.iconBadge}>
              <Icon icon={boltIcon} size="1.2em" />
            </span>
            <div>
              <h3>Nexusor Akıllı Model Motoru ve Otomatik Failover</h3>
              <p>Herhangi bir model seçildiğinde (örn: Gemini veya Claude), o modelin kotası biterse veya 429 alınırsa Nexusor istek bazında otomatik olarak sıradaki sağlıklı hesaba geçer.</p>
            </div>
          </div>
          <Switch
            checked={autoConfig.enabled}
            label={autoConfig.enabled ? "Auto Router: Aktif" : "Auto Router: Kapalı"}
            onChange={toggleAutoRouter}
          />
        </div>

        <div className={styles.autoGrid}>
          {/* 1. Kodlama */}
          <div className={styles.autoCategory}>
            <span className={styles.catTitle}>Kodlama Görevleri (Coding)</span>
            <span className={styles.catDesc}>Kod yazma, düzenleme ve dosya analizleri</span>
            <select
              className={styles.selectBox}
              value={autoConfig.coding_slots[0]?.model_id || ""}
              onChange={(e) => updateAutoPrimaryModel("coding", e.target.value)}
            >
              <option value="">(Otomatik - En Uygun Modeli Seç)</option>
              {accountsList.map((acc) => (
                <optgroup key={acc.id} label={acc.name}>
                  {(modelsByAccount.get(acc.id) || []).map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name}
                    </option>
                  ))}
                </optgroup>
              ))}
            </select>
          </div>

          {/* 2. Düşünme */}
          <div className={styles.autoCategory}>
            <span className={styles.catTitle}>Derin Akıl Yürütme (Reasoning)</span>
            <span className={styles.catDesc}>Karmaşık mantık, mimari ve planlama</span>
            <select
              className={styles.selectBox}
              value={autoConfig.reasoning_slots[0]?.model_id || ""}
              onChange={(e) => updateAutoPrimaryModel("reasoning", e.target.value)}
            >
              <option value="">(Otomatik - Düşünme Modelini Seç)</option>
              {accountsList.map((acc) => (
                <optgroup key={acc.id} label={acc.name}>
                  {(modelsByAccount.get(acc.id) || []).map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name}
                    </option>
                  ))}
                </optgroup>
              ))}
            </select>
          </div>

          {/* 3. Hızlı */}
          <div className={styles.autoCategory}>
            <span className={styles.catTitle}>Hızlı Tamamlama & Tab (Fast)</span>
            <span className={styles.catDesc}>Satır içi tamamlama ve anlık sorular</span>
            <select
              className={styles.selectBox}
              value={autoConfig.fast_slots[0]?.model_id || ""}
              onChange={(e) => updateAutoPrimaryModel("fast", e.target.value)}
            >
              <option value="">(Otomatik - En Hızlı Modeli Seç)</option>
              {accountsList.map((acc) => (
                <optgroup key={acc.id} label={acc.name}>
                  {(modelsByAccount.get(acc.id) || []).map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name}
                    </option>
                  ))}
                </optgroup>
              ))}
            </select>
          </div>
        </div>
      </Card>

      {/* ── BÖLÜM 2: Özel Yedekleme Zincirleri (Failover & Round-Robin) ── */}
      <div style={{ display: "flex", flexDirection: "column", gap: "12px" }}>
        <div className={styles.sectionHeading}>
          <div>
            <h3>Özel Sıralı Yedekleme Zincirleri (Failover Chains)</h3>
            <p>Belirli modeller arasında zorunlu bir geçiş zinciri kurmak için (Örn: Claude biterse Gemini devreye girsin).</p>
          </div>
          <Button
            variant={showCreateCombo ? "secondary" : "primary"}
            size="small"
            onClick={() => setShowCreateCombo((prev) => !prev)}
          >
            {showCreateCombo ? "İptal Et" : "+ Yeni Zincir Oluştur"}
          </Button>
        </div>

        {/* Yeni Zincir Oluşturma Formu (Hesaplara Göre Gruplu ve Düzenli) */}
        {showCreateCombo && (
          <div className={styles.createCard}>
            <div className={styles.createRow}>
              <input
                type="text"
                placeholder="Zincir Adı (Örn: Claude Sonnet -> Gemini 2.5 Pro)"
                value={comboName}
                onChange={(e) => setComboName(e.target.value)}
              />
              <select
                value={comboStrategy}
                onChange={(e) => setComboStrategy(e.target.value as "fallback" | "round_robin")}
              >
                <option value="fallback">Failover (Biri biterse sıradakine geç)</option>
                <option value="round_robin">Round-Robin (Her istekte sırayla döndür)</option>
              </select>
            </div>

            {/* Hesap Seçim Sekmeleri */}
            <div style={{ display: "flex", gap: "6px", flexWrap: "wrap", borderBottom: "1px solid rgba(255,255,255,0.06)", paddingBottom: "10px" }}>
              <button
                type="button"
                style={{
                  padding: "4px 10px",
                  fontSize: "12px",
                  borderRadius: "4px",
                  border: "none",
                  cursor: "pointer",
                  background: activeAccountFilter === "all" ? "rgba(255,255,255,0.15)" : "transparent",
                  color: activeAccountFilter === "all" ? "#fff" : "rgba(255,255,255,0.6)",
                }}
                onClick={() => setActiveAccountFilter("all")}
              >
                Tüm Hesaplar
              </button>
              {accountsList.map((acc) => (
                <button
                  key={acc.id}
                  type="button"
                  style={{
                    padding: "4px 10px",
                    fontSize: "12px",
                    borderRadius: "4px",
                    border: "none",
                    cursor: "pointer",
                    background: activeAccountFilter === acc.id ? "rgba(255,255,255,0.15)" : "transparent",
                    color: activeAccountFilter === acc.id ? "#fff" : "rgba(255,255,255,0.6)",
                  }}
                  onClick={() => setActiveAccountFilter(acc.id)}
                >
                  {acc.name}
                </button>
              ))}
            </div>

            {/* Modeller Listesi */}
            <div className={styles.slotPicker}>
              <span className={styles.slotPickerLabel}>
                Zincire Eklenecek Modelleri Sırayla Tıklayın ({selectedModelList.length} seçildi):
              </span>
              <div className={styles.slotChips}>
                {accountsList
                  .filter((acc) => activeAccountFilter === "all" || activeAccountFilter === acc.id)
                  .flatMap((acc) =>
                    (modelsByAccount.get(acc.id) || []).map((m) => {
                      const isSelected = selectedModelList.includes(m.id);
                      const orderIdx = selectedModelList.indexOf(m.id);
                      return (
                        <button
                          key={m.id}
                          type="button"
                          className={`${styles.slotChip} ${isSelected ? styles.slotChipSelected : ""}`}
                          onClick={() => toggleModelInNewCombo(m.id)}
                        >
                          {isSelected && <strong>{orderIdx + 1}.</strong>}
                          <span>{m.name}</span>
                          <span style={{ fontSize: "10px", opacity: 0.5, marginLeft: 4 }}>({acc.name})</span>
                        </button>
                      );
                    })
                  )}
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
