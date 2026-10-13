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
import { getProviderLogo } from "../../shared/utils/providerIcons";
import styles from "./CombosPage.module.scss";

type FallbackPolicy = "account_pool" | "model_fallback" | "strict_stop";

export function CombosPage() {
  const message = useMessage();
  const { plugins, models } = useAppStore();

  const [combos, setCombos] = useState<RouterCombo[]>([]);
  const [deletingCombo, setDeletingCombo] = useState<RouterCombo | null>(null);

  // Auto Router Config
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

  // Kullanıcının belirlediği Hata Anı Politikası (Default Fallback Policy)
  const [fallbackPolicy, setFallbackPolicy] = useState<FallbackPolicy>(() => {
    return (localStorage.getItem("nexusor_fallback_policy") as FallbackPolicy) || "account_pool";
  });

  // İkinci modele devretme için seçilen model
  const [secondaryFallbackModel, setSecondaryFallbackModel] = useState<string>(() => {
    return localStorage.getItem("nexusor_secondary_fallback_model") || "";
  });

  // Özel zincir oluşturma durumu
  const [showCreateCombo, setShowCreateCombo] = useState(false);
  const [comboName, setComboName] = useState("");
  const [comboStrategy, setComboStrategy] = useState<"fallback" | "round_robin">("fallback");
  const [selectedModelList, setSelectedModelList] = useState<string[]>([]);
  const [chainProviderFilter, setChainProviderFilter] = useState<string>("all");

  // Hiyerarşik Sağlayıcı ve Model Haritası
  const { providerGroups } = useMemo(() => {
    const groups: Array<{
      id: string;
      name: string;
      logo: string | null;
      accountCount: number;
      models: Array<{ id: string; name: string; rawId: string }>;
    }> = [];

    const flat: Array<{ id: string; name: string; providerName: string }> = [];

    // 1. Eklenti Sağlayıcıları
    const pModels = configuredPluginModels(plugins);
    for (const p of plugins) {
      const pModelsFiltered = pModels.filter((m) => m.pluginId === p.id);
      if (pModelsFiltered.length > 0) {
        const accountCount = p.resources.reduce((c, r) => c + r.resources.length, 0);
        const groupModels = pModelsFiltered.map((m) => ({
          id: m.id,
          name: m.displayName || m.modelId,
          rawId: m.modelId,
        }));

        groups.push({
          id: p.id,
          name: p.name,
          logo: getProviderLogo(p.id) || getProviderLogo(p.name),
          accountCount,
          models: groupModels,
        });

        for (const gm of groupModels) {
          flat.push({ id: gm.id, name: gm.name, providerName: p.name });
        }
      }
    }

    // 2. Özel / Manuel API Sağlayıcıları
    if (models.length > 0) {
      const manModels = models.map((m) => ({
        id: m.model_id,
        name: m.display_name || m.model_id,
        rawId: m.model_id,
      }));
      groups.push({
        id: "manual-api",
        name: "Özel API / BYOK Modelleri",
        logo: null,
        accountCount: 1,
        models: manModels,
      });

      for (const mm of manModels) {
        flat.push({ id: mm.id, name: mm.name, providerName: "Özel API" });
      }
    }

    return { providerGroups: groups };
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

  const handlePolicyChange = (policy: FallbackPolicy) => {
    setFallbackPolicy(policy);
    localStorage.setItem("nexusor_fallback_policy", policy);
    message("Model hata anı politikası güncellendi");
  };

  const handleSecondaryModelChange = (modelId: string) => {
    setSecondaryFallbackModel(modelId);
    localStorage.setItem("nexusor_secondary_fallback_model", modelId);
    message("İkincil yedek model güncellendi");
  };

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

  const updateCategorySlot = async (
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
      message("Kategori modeli güncellendi");
    } catch (e) {
      message(String(e));
    }
  };

  const toggleModelInChain = (id: string) => {
    setSelectedModelList((prev) =>
      prev.includes(id) ? prev.filter((m) => m !== id) : [...prev, id]
    );
  };

  const saveChain = async () => {
    if (!comboName.trim()) {
      message("Lütfen zincir için bir ad girin");
      return;
    }
    if (selectedModelList.length < 2) {
      message("Bir zincir en az 2 model içermelidir");
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
      message("Özel yedekleme zinciri kaydedildi");
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
      {/* ── 1. GÖRSEL İSTEK YAŞAM DÖNGÜSÜ HATTI (PIPELINE FLOW) ── */}
      <Card className={styles.pipelineCard}>
        <div className={styles.pipelineHeader}>
          <div className={styles.headerTitleGroup}>
            <span className={styles.iconBadge}>
              <Icon icon={boltIcon} size="1.3em" />
            </span>
            <div>
              <h3>Model İstek Yaşam Döngüsü & Hata Politikası</h3>
              <p>Modelinizin kotası bittiğinde veya hata aldığında ne olacağını siz belirlersiniz.</p>
            </div>
          </div>
          <Switch
            checked={autoConfig.enabled}
            label={autoConfig.enabled ? "Auto Router: Aktif" : "Auto Router: Kapalı"}
            onChange={toggleAutoRouter}
          />
        </div>

        {/* Görsel Akış Şeması (Pipeline Nodes) */}
        <div className={styles.pipelineFlow}>
          {/* Adım 1: Birincil Model */}
          <div className={styles.pipelineNode}>
            <span className={styles.nodeStep}>Adım 1: Birincil İstek</span>
            <span className={styles.nodeTitle}>Hedeflenen Model</span>
            <span className={styles.nodeDesc}>İstemcinin (Cursor/Cline) seçtiği model</span>
            <select
              className={styles.selectBox}
              value={autoConfig.coding_slots[0]?.model_id || ""}
              onChange={(e) => updateCategorySlot("coding", e.target.value)}
            >
              <option value="">(İstemcinin Kendi Seçimi)</option>
              {providerGroups.map((g) => (
                <optgroup key={g.id} label={g.name}>
                  {g.models.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name}
                    </option>
                  ))}
                </optgroup>
              ))}
            </select>
          </div>

          <div className={styles.flowArrow}>
            <Icon icon={arrowRightIcon} size="1.4em" />
          </div>

          {/* Adım 2: Hata Karar Noktası */}
          <div className={styles.pipelineNode}>
            <span className={styles.nodeStep}>Adım 2: Hata Anı Politikası</span>
            <span className={styles.nodeTitle}>Kota / 429 Hata Davranışı</span>
            <div className={styles.policyOptions}>
              <button
                type="button"
                className={`${styles.policyBtn} ${fallbackPolicy === "account_pool" ? styles.policyBtnActive : ""}`}
                onClick={() => handlePolicyChange("account_pool")}
              >
                <span>Aynı Modelin Yedek Hesabına Geç</span>
                {fallbackPolicy === "account_pool" && <strong>Aktif</strong>}
              </button>
              <button
                type="button"
                className={`${styles.policyBtn} ${fallbackPolicy === "model_fallback" ? styles.policyBtnActive : ""}`}
                onClick={() => handlePolicyChange("model_fallback")}
              >
                <span>Belirlediğim Sıradaki Modele Devret</span>
                {fallbackPolicy === "model_fallback" && <strong>Aktif</strong>}
              </button>
              <button
                type="button"
                className={`${styles.policyBtn} ${fallbackPolicy === "strict_stop" ? styles.policyBtnActive : ""}`}
                onClick={() => handlePolicyChange("strict_stop")}
              >
                <span>Katı Durdur (Hata Göster)</span>
                {fallbackPolicy === "strict_stop" && <strong>Aktif</strong>}
              </button>
            </div>
          </div>

          <div className={styles.flowArrow}>
            <Icon icon={arrowRightIcon} size="1.4em" />
          </div>

          {/* Adım 3: Sonuç / Yedek Hedef */}
          <div className={styles.pipelineNode}>
            <span className={styles.nodeStep}>Adım 3: Sonuç Eylemi</span>
            <span className={styles.nodeTitle}>Yedek Hedef</span>
            {fallbackPolicy === "strict_stop" ? (
              <span className={styles.nodeDesc} style={{ color: "#ef4444" }}>
                İşlem anında kesilir, hiçbir modele devredilmez.
              </span>
            ) : fallbackPolicy === "account_pool" ? (
              <span className={styles.nodeDesc} style={{ color: "#27c376" }}>
                Aynı model sağlayıcısındaki sıradaki sağlıklı hesap denenir.
              </span>
            ) : (
              <select
                className={styles.selectBox}
                value={secondaryFallbackModel}
                onChange={(e) => handleSecondaryModelChange(e.target.value)}
              >
                <option value="">(Yedek Model Seçin)</option>
                {providerGroups.map((g) => (
                  <optgroup key={g.id} label={g.name}>
                    {g.models.map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.name}
                      </option>
                    ))}
                  </optgroup>
                ))}
              </select>
            )}
          </div>
        </div>
      </Card>

      {/* ── 2. SAĞLAYICI VE HESAP HİYERARŞİSİ (PROVIDER ACCORDION / TREE) ── */}
      <div className={styles.providersSection}>
        <div className={styles.sectionHeaderRow}>
          <div>
            <h3>Sağlayıcı ve Model Envanteri</h3>
            <p>Hesaplarınıza bağlı modeller ve durumları düzenli hiyerarşiyle listelenir.</p>
          </div>
        </div>

        <div className={styles.providerTreeGrid}>
          {providerGroups.map((group) => (
            <div key={group.id} className={styles.providerTreeCard}>
              <div className={styles.providerTreeHeader}>
                <div className={styles.providerNameBadge}>
                  {group.logo && <img src={group.logo} alt="" style={{ width: 20, height: 20, objectFit: "contain" }} />}
                  <strong>{group.name}</strong>
                </div>
                <span className={styles.modelCountTag}>{group.models.length} Model</span>
              </div>

              <div className={styles.modelTreeList}>
                {group.models.map((m) => (
                  <div key={m.id} className={styles.modelTreeRow}>
                    <span className={styles.modelNameText}>{m.name}</span>
                    <span className={styles.modelIdMono}>{cleanName(m.rawId)}</span>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* ── 3. ÖZEL YEDEKLEME ZİNCİRLERİ (CHAINS) ── */}
      <div style={{ display: "flex", flexDirection: "column", gap: "14px" }}>
        <div className={styles.sectionHeaderRow}>
          <div>
            <h3>Özel Sıralı Zincirler (Failover & Round-Robin)</h3>
            <p>Spesifik modeller arasında sıralı yedekleme veya yük dağılımı zincirleri tanımlayın.</p>
          </div>
          <Button
            variant={showCreateCombo ? "secondary" : "primary"}
            size="small"
            onClick={() => setShowCreateCombo((prev) => !prev)}
          >
            {showCreateCombo ? "İptal Et" : "+ Yeni Zincir Tanımla"}
          </Button>
        </div>

        {/* Yeni Zincir Tanımlama Formu */}
        {showCreateCombo && (
          <div className={styles.createForm}>
            <div className={styles.createInputs}>
              <input
                type="text"
                placeholder="Zincir Adı (Örn: Claude 3.7 -> Gemini 2.5 Pro)"
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

            {/* Sağlayıcı Filtresi */}
            <div style={{ display: "flex", gap: "6px", flexWrap: "wrap" }}>
              <button
                type="button"
                style={{
                  padding: "4px 10px",
                  fontSize: "12px",
                  borderRadius: "4px",
                  border: "none",
                  cursor: "pointer",
                  background: chainProviderFilter === "all" ? "rgba(255,255,255,0.15)" : "transparent",
                  color: chainProviderFilter === "all" ? "#fff" : "rgba(255,255,255,0.6)",
                }}
                onClick={() => setChainProviderFilter("all")}
              >
                Tümü
              </button>
              {providerGroups.map((g) => (
                <button
                  key={g.id}
                  type="button"
                  style={{
                    padding: "4px 10px",
                    fontSize: "12px",
                    borderRadius: "4px",
                    border: "none",
                    cursor: "pointer",
                    background: chainProviderFilter === g.id ? "rgba(255,255,255,0.15)" : "transparent",
                    color: chainProviderFilter === g.id ? "#fff" : "rgba(255,255,255,0.6)",
                  }}
                  onClick={() => setChainProviderFilter(g.id)}
                >
                  {g.name}
                </button>
              ))}
            </div>

            {/* Tıklanabilir Model Çipleri */}
            <div className={styles.chipsContainer}>
              {providerGroups
                .filter((g) => chainProviderFilter === "all" || chainProviderFilter === g.id)
                .flatMap((g) =>
                  g.models.map((m) => {
                    const isSelected = selectedModelList.includes(m.id);
                    const orderIdx = selectedModelList.indexOf(m.id);
                    return (
                      <button
                        key={m.id}
                        type="button"
                        className={`${styles.chipBtn} ${isSelected ? styles.chipBtnSelected : ""}`}
                        onClick={() => toggleModelInChain(m.id)}
                      >
                        {isSelected && <strong>{orderIdx + 1}.</strong>}
                        <span>{m.name}</span>
                        <small style={{ opacity: 0.5, marginLeft: 4 }}>({g.name})</small>
                      </button>
                    );
                  })
                )}
            </div>

            <div style={{ display: "flex", justifyContent: "flex-end", gap: "8px" }}>
              <Button size="small" variant="secondary" onClick={() => setShowCreateCombo(false)}>
                Vazgeç
              </Button>
              <Button size="small" variant="primary" onClick={saveChain}>
                Zinciri Kaydet
              </Button>
            </div>
          </div>
        )}

        {/* Kayıtlı Zincirler */}
        {combos.length === 0 ? (
          <Card style={{ padding: "28px", textAlign: "center", background: "#17181c", border: "1px dashed rgba(255,255,255,0.08)", borderRadius: "8px" }}>
            <p style={{ margin: 0, color: "rgba(255,255,255,0.45)", fontSize: "13px" }}>
              Henüz tanımlanmış özel bir yedekleme zinciri bulunmuyor.
            </p>
          </Card>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: "10px" }}>
            {combos.map((combo) => (
              <div key={combo.combo_id} className={styles.chainRow}>
                <div className={styles.chainLeft}>
                  <div className={styles.dot} data-active={combo.enabled ? "true" : "false"} />
                  <div>
                    <h4 className={styles.chainName}>{combo.name}</h4>
                    <span className={styles.chainId}>combo:{combo.combo_id}</span>
                  </div>
                </div>

                <div className={styles.chainNodes}>
                  {(combo.models || []).map((mid, idx) => (
                    <div key={idx} style={{ display: "flex", alignItems: "center", gap: "6px" }}>
                      <span className={styles.chainPill}>{cleanName(mid)}</span>
                      {idx < (combo.models || []).length - 1 && (
                        <Icon icon={arrowRightIcon} size="0.9em" style={{ color: "rgba(255,255,255,0.25)" }} />
                      )}
                    </div>
                  ))}
                </div>

                <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
                  <Switch
                    checked={combo.enabled}
                    label={combo.enabled ? "Aktif" : "Pasif"}
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
        sections={[{ key: "routing-pipeline", estimatedHeight: 800, content }]}
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
        <p>"{deletingCombo?.name}" zincirini silmek istediğinize emin misiniz?</p>
      </ConfirmDialog>
    </>
  );
}
