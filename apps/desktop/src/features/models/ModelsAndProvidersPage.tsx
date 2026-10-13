import { useState } from "react";
import { CursorSettingsPage } from "../models/CursorSettingsPage";
import { ProvidersPage } from "../providers/ProvidersPage";
import { useI18n } from "../../i18n/store";
import styles from "./ModelsAndProvidersPage.module.scss";

export function ModelsAndProvidersPage() {
  // Kullanıcı tercihi: Sağlayıcılar ilk sekmede
  const [activeTab, setActiveTab] = useState<"providers" | "models">("providers");
  useI18n();

  return (
    <div className={styles.container}>
      {/* ── Üst Başlık ve Temiz Sekme Çubuğu (Çift Başlık Sorunu Çözüldü) ── */}
      <div className={styles.topNav}>
        <div className={styles.header}>
          <div className={styles.tabGroup}>
            <button
              type="button"
              className={`${styles.tabBtn} ${activeTab === "providers" ? styles.tabBtnActive : ""}`}
              onClick={() => setActiveTab("providers")}
            >
              {t("nav.providers")}
            </button>
            <button
              type="button"
              className={`${styles.tabBtn} ${activeTab === "models" ? styles.tabBtnActive : ""}`}
              onClick={() => setActiveTab("models")}
            >
              {t("models.models")}
            </button>
          </div>
        </div>
      </div>

      {/* ── İçerik Alanı ── */}
      <div className={styles.contentArea}>
        {activeTab === "providers" ? <ProvidersPage /> : <CursorSettingsPage />}
      </div>
    </div>
  );
}
