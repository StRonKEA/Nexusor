import { useState } from "react";
import { CursorSettingsPage } from "../models/CursorSettingsPage";
import { ProvidersPage } from "../providers/ProvidersPage";
import { useI18n } from "../../i18n/store";
import styles from "./ModelsAndProvidersPage.module.scss";

export function ModelsAndProvidersPage() {
  const [activeTab, setActiveTab] = useState<"models" | "providers">("models");
  useI18n();

  const titleHeader = (
    <div className={styles.header}>
      <span className={styles.pageTitle}>{t("nav.models_and_providers")}</span>
      <div className={styles.tabGroup}>
        <button
          type="button"
          className={`${styles.tabBtn} ${activeTab === "models" ? styles.tabBtnActive : ""}`}
          onClick={() => setActiveTab("models")}
        >
          {t("models.models")}
        </button>
        <button
          type="button"
          className={`${styles.tabBtn} ${activeTab === "providers" ? styles.tabBtnActive : ""}`}
          onClick={() => setActiveTab("providers")}
        >
          {t("nav.providers")}
        </button>
      </div>
    </div>
  );

  return (
    <div className={styles.container}>
      <div className={styles.topNav}>
        {titleHeader}
      </div>
      <div className={styles.contentArea}>
        {activeTab === "models" ? <CursorSettingsPage /> : <ProvidersPage />}
      </div>
    </div>
  );
}
