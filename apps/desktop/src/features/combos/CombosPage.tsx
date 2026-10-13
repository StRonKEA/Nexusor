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

type FallbackPolicy = "account_pool" | "model_fallback" | "strict_stop";

export function CombosPage() {
  const message = useMessage();
  const { plugins, models } = useAppStore();

  const [combos, setCombos] = useState<RouterCombo[]>([]);
  const [deletingCombo, setDeletingCombo] = useState<RouterCombo | null>(null);

  // 1. Cursor Auto Router Konfigürasyonu
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

  // 2. Kullanıcının Varsayılan Hata / Kota Politikası
  const [fallbackPolicy, setFallbackPolicy] = useState<FallbackPolicy>(() => {
    return (localStorage.getItem("nexusor_fallback_policy") as FallbackPolicy) || "account_pool";
  });

  // 3. Yeni Özel Zincir Oluşturma Formu (Adım Adım Basit Sıralama)
  const [showCreateChain, setShowCreateChain] = useState(false);
  const [chainName, setChainName] = useState("");
  const [chainStrategy, setChainStrategy] = useState<"fallback" | "round_robin">("fallback");
  const [chainSteps, setChainSteps] = useState<string[]>(["", ""]);

  // Modelleri ait oldukları hesap / sağlayıcı başlığı altında grupla
  const groupedModels = useMemo(() => {
    const map = new Map<string, Array<{ id: string; label: string }>>();

    // Eklenti Sağlayıcıları (Google, OpenAI, Copilot, Grok vb.)
    const pModels = configuredPluginModels(plugins);
    for (const pm of pModels) {
      const providerKey = pm.pluginName || "Dahili Hesaplar";
      const existing = map.get(providerKey) || [];
      existing.push({ id: pm.id, label: pm.displayName || pm.modelId });
      map.set(providerKey, existing);
    }

    // Manuel BYOK Modelleri
    if (models.length > 0) {
      const manualKey = "Özel API / BYOK Modelleri";
      const existing = map.get(manualKey) || [];
      for (const mm of models) {
        existing.push({ id: mm.model_id, label: mm.display_name || mm.model_id });
      }
      map.set(manualKey, existing);
    }

    return Array.from(map.entries()).map(([provider, items]) => ({
      provider,
      items,
    }));
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

  // Auto Router açma/kapama
  const toggleAutoRouter = async (enabled: boolean) => {
    try {
      const updated = { ...autoConfig, enabled, updated_at_ms: Date.now() };
      await api.saveAutoRouterConfig(updated);
      setAutoConfig(updated);
      await api.setModelCustomization("auto-smart", { custom_name: null, enabled });
      message(enabled ? "Cursor Auto Modu Etkinleştirildi" : "Cursor Auto Modu Kapatıldı");
    } catch (e) {
      message(String(e));
    }
  };

  // Auto Router kategori modeli güncelleme (Kodlama, Düşünme, Hızlı)
  const setCategoryModel = async (
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
      message("Auto model seçimi güncellendi");
    } catch (e) {
      message(String(e));
    }
  };

  const handlePolicyChange = (policy: FallbackPolicy) => {
    setFallbackPolicy(policy);
    localStorage.setItem("nexusor_fallback_policy", policy);
    message("Hata anı politikası güncellendi");
  };

  // Zincir adım yönetimi
  const addChainStep = () => {
    setChainSteps((prev) => [...prev, ""]);
  };

  const updateChainStep = (index: number, modelId: string) => {
    setChainSteps((prev) => {
      const next = [...prev];
      next[index] = modelId;
      return next;
    });
  };

  const removeChainStep = (index: number) => {
    if (chainSteps.length <= 2) {
      message("Bir zincir en az 2 model içermelidir");
      return;
    }
    setChainSteps((prev) => prev.filter((_, idx) => idx !== index));
  };

  const saveChain = async () => {
    if (!chainName.trim()) {
      message("Lütfen zincir için bir isim girin");
      return;
    }
    const cleanList = chainSteps.filter((id) => id.trim() !== "");
    if (cleanList.length < 2) {
      message("Lütfen zincir için en az 2 geçerli model seçin");
      return;
    }

    try {
      const now = Date.now();
      const newCombo: RouterCombo = {
        combo_id: `combo-${now}`,
        name: chainName.trim(),
        description: "",
        strategy: chainStrategy,
        enabled: true,
        models: cleanList,
        slots: cleanList.map((m) => ({
          model_id: m,
          account_ids: null,
          slot_strategy: chainStrategy,
        })),
        created_at_ms: now,
        updated_at_ms: now,
      };

      await api.upsertRouterCombo(newCombo);
      await loadData();
      setShowCreateChain(false);
      setChainName("");
      setChainSteps(["", ""]);
      message("Özel yedekleme zinciri başarıyla oluşturuldu");
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
      {/* ── BÖLÜM 1: Cursor Auto Modu (Akıllı Yönlendirici) ── */}
      <Card className={styles.card}>
        <div className={styles.cardHeader}>
          <div className={styles.titleGroup}>
            <span className={styles.iconBadge}>
              <Icon icon={boltIcon} size="1.2em" />
            </span>
            <div>
              <h3>Cursor Auto Modu (Akıllı Yönlendirici)</h3>
              <p>Cursor içinde veya harici araçta <code>auto-smart</code> seçildiğinde görev tipine göre hangi modelin çalışacağını belirleyin.</p>
            </div>
          </div>
          <Switch
            checked={autoConfig.enabled}
            label={autoConfig.enabled ? "Auto Mod: Açık" : "Auto Mod: Kapalı"}
            onChange={toggleAutoRouter}
          />
        </div>

        <div className={styles.autoGrid}>
          {/* Kodlama */}
          <div className={styles.autoCategory}>
            <span className={styles.catTitle}>Kodlama Görevleri (Coding)</span>
            <span className={styles.catDesc}>Kod yazma, çok dosyalı düzenleme ve dosya analizleri</span>
            <select
              className={styles.selectBox}
              value={autoConfig.coding_slots[0]?.model_id || ""}
              onChange={(e) => setCategoryModel("coding", e.target.value)}
            >
              <option value="">(Otomatik - En Uygun Modeli Belirle)</option>
              {groupedModels.map((g) => (
                <optgroup key={g.provider} label={g.provider}>
                  {g.items.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.label}
                    </option>
                  ))}
                </optgroup>
              ))}
            </select>
          </div>

          {/* Düşünme */}
          <div className={styles.autoCategory}>
            <span className={styles.catTitle}>Derin Akıl Yürütme (Reasoning)</span>
            <span className={styles.catDesc}>Karmaşık mimari planlama ve mantık yürütme</span>
            <select
              className={styles.selectBox}
              value={autoConfig.reasoning_slots[0]?.model_id || ""}
              onChange={(e) => setCategoryModel("reasoning", e.target.value)}
            >
              <option value="">(Otomatik - Düşünme Modelini Belirle)</option>
              {groupedModels.map((g) => (
                <optgroup key={g.provider} label={g.provider}>
                  {g.items.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.label}
                    </option>
                  ))}
                </optgroup>
              ))}
            </select>
          </div>

          {/* Hızlı */}
          <div className={styles.autoCategory}>
            <span className={styles.catTitle}>Hızlı Tamamlama & Tab (Fast)</span>
            <span className={styles.catDesc}>Satır içi kod tamamlama ve kısa sorular</span>
            <select
              className={styles.selectBox}
              value={autoConfig.fast_slots[0]?.model_id || ""}
              onChange={(e) => setCategoryModel("fast", e.target.value)}
            >
              <option value="">(Otomatik - En Hızlı Modeli Belirle)</option>
              {groupedModels.map((g) => (
                <optgroup key={g.provider} label={g.provider}>
                  {g.items.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.label}
                    </option>
                  ))}
                </optgroup>
              ))}
            </select>
          </div>
        </div>
      </Card>

      {/* ── BÖLÜM 2: Model Hata ve Kota Politikası (Kullanıcı Seçimi) ── */}
      <Card className={styles.card}>
        <div className={styles.cardHeader}>
          <div className={styles.titleGroup}>
            <div>
              <h3>Model Kotası / Hata Durumunda Ne Yapılsın?</h3>
              <p>Tekil bir model kullanırken (örn: Gemini 3.7) kotası dolarsa veya 429 alınırsa sistemin izleyeceği kural.</p>
            </div>
          </div>
        </div>

        <div className={styles.policyGrid}>
          <button
            type="button"
            className={`${styles.policyCard} ${fallbackPolicy === "account_pool" ? styles.policyCardActive : ""}`}
            onClick={() => handlePolicyChange("account_pool")}
          >
            <span className={styles.policyTitle}>Aynı Modelin Sıradaki Hesabına Geç (Önerilen)</span>
            <span className={styles.policyDesc}>Mevcut hesabın kotası biterse aynı modelin bağlı diğer hesabına kesintisiz devredilir.</span>
          </button>

          <button
            type="button"
            className={`${styles.policyCard} ${fallbackPolicy === "model_fallback" ? styles.policyCardActive : ""}`}
            onClick={() => handlePolicyChange("model_fallback")}
          >
            <span className={styles.policyTitle}>Özel Yedekleme Zincirine Devret</span>
            <span className={styles.policyDesc}>Hesaplar biterse aşağıda tanımladığınız özel yedekleme zincirindeki sıradaki modele geçer.</span>
          </button>

          <button
            type="button"
            className={`${styles.policyCard} ${fallbackPolicy === "strict_stop" ? styles.policyCardActive : ""}`}
            onClick={() => handlePolicyChange("strict_stop")}
          >
            <span className={styles.policyTitle}>Katı Durdur (Hata Göster)</span>
            <span className={styles.policyDesc}>Kota biterse hiçbir yere devretme; anında işlemi durdurup hatayı ekrana bas.</span>
          </button>
        </div>
      </Card>

      {/* ── BÖLÜM 3: Özel Sıralı Yedekleme Zincirleri (Failover Chains) ── */}
      <div style={{ display: "flex", flexDirection: "column", gap: "12px" }}>
        <div className={styles.sectionHeader}>
          <div>
            <h3>Özel Yedekleme Zincirleri (Failover Chains)</h3>
            <p>Spesifik modeller arasında adım adım sıralı bir yedekleme kuralı tanımlayın.</p>
          </div>
          <Button
            variant={showCreateChain ? "secondary" : "primary"}
            size="small"
            onClick={() => setShowCreateChain((prev) => !prev)}
          >
            {showCreateChain ? "Vazgeç" : "+ Yeni Yedekleme Zinciri Ekle"}
          </Button>
        </div>

        {/* Adım Adım Temiz Zincir Oluşturucu */}
        {showCreateChain && (
          <div className={styles.createCard}>
            <div className={styles.createInputs}>
              <input
                type="text"
                placeholder="Zincir Adı (Örn: Claude Sonnet -> Gemini 2.5 Pro)"
                value={chainName}
                onChange={(e) => setChainName(e.target.value)}
              />
              <select
                value={chainStrategy}
                onChange={(e) => setChainStrategy(e.target.value as "fallback" | "round_robin")}
              >
                <option value="fallback">Failover (Sırayla Yedekle - Biri biterse sıradakine geç)</option>
                <option value="round_robin">Round-Robin (Her istekte sırayla modeli döndür)</option>
              </select>
            </div>

            {/* Adım Adım Model Seçim Satırları */}
            <div className={styles.stepList}>
              {chainSteps.map((selectedMid, idx) => (
                <div key={idx} className={styles.stepRow}>
                  <span className={styles.stepNum}>{idx + 1}.</span>
                  <select
                    className={styles.stepSelect}
                    value={selectedMid}
                    onChange={(e) => updateChainStep(idx, e.target.value)}
                  >
                    <option value="">(Bir model seçin)</option>
                    {groupedModels.map((g) => (
                      <optgroup key={g.provider} label={g.provider}>
                        {g.items.map((m) => (
                          <option key={m.id} value={m.id}>
                            {m.label}
                          </option>
                        ))}
                      </optgroup>
                    ))}
                  </select>
                  <button
                    type="button"
                    className={styles.removeStepBtn}
                    onClick={() => removeChainStep(idx)}
                    title="Bu adımı kaldır"
                  >
                    ✕
                  </button>
                </div>
              ))}
              <button type="button" className={styles.addStepBtn} onClick={addChainStep}>
                + Sıradaki Yedek Modeli Ekle ({chainSteps.length + 1}. Adım)
              </button>
            </div>

            <div style={{ display: "flex", justifyContent: "flex-end", gap: "8px" }}>
              <Button size="small" variant="secondary" onClick={() => setShowCreateChain(false)}>
                Vazgeç
              </Button>
              <Button size="small" variant="primary" onClick={saveChain}>
                Zinciri Kaydet
              </Button>
            </div>
          </div>
        )}

        {/* Mevcut Zincirler */}
        {combos.length === 0 ? (
          <Card style={{ padding: "30px", textAlign: "center", background: "#17181c", border: "1px dashed rgba(255,255,255,0.08)", borderRadius: "8px" }}>
            <p style={{ margin: 0, color: "rgba(255,255,255,0.45)", fontSize: "13px" }}>
              Henüz tanımlanmış bir yedekleme zinciri yok. İhtiyaç duyduğunuzda yukarıdaki butondan ekleyebilirsiniz.
            </p>
          </Card>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: "10px" }}>
            {combos.map((combo) => (
              <div key={combo.combo_id} className={styles.chainCard}>
                <div className={styles.chainLeft}>
                  <div className={styles.dot} data-active={combo.enabled ? "true" : "false"} />
                  <div>
                    <h4 className={styles.chainName}>{combo.name}</h4>
                    <span className={styles.chainId}>combo:{combo.combo_id}</span>
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

                <div className={styles.chainActions}>
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
        sections={[{ key: "combos-page", estimatedHeight: 800, content }]}
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
