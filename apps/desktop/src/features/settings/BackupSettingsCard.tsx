import { useRef, useState } from "react";
import { Button } from "../../shared/ui/Button";
import { TitledCard } from "../../shared/ui/TitledCard";
import { useMessage } from "../../shared/ui/message";
import { api } from "../../shared/api";
import { appStore } from "../../shared/store/appStore";
import styles from "./SettingsPage.module.scss";

export function BackupSettingsCard() {
  const message = useMessage();
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [exporting, setExporting] = useState(false);
  const [restoring, setRestoring] = useState(false);

  const handleExport = async () => {
    try {
      setExporting(true);
      const data = await api.exportBackup();
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const dateStr = new Date().toISOString().slice(0, 10);
      const link = document.createElement("a");
      link.href = url;
      link.download = `nexusor-backup-${dateStr}.json`;
      document.body.appendChild(link);
      link.click();
      document.body.removeChild(link);
      URL.revokeObjectURL(url);
      message(t("settings.backup_export_success"));
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      message(msg || t("settings.backup_export_failed"));
    } finally {
      setExporting(false);
    }
  };

  const handleFileChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    try {
      setRestoring(true);
      const text = await file.text();
      let parsed: Record<string, unknown>;
      try {
        parsed = JSON.parse(text) as Record<string, unknown>;
      } catch {
        message(t("settings.backup_restore_invalid"));
        return;
      }

      const res = await api.restoreBackup(parsed);
      void appStore.refresh();
      if (res.success) {
        message(
          t("settings.backup_restore_success", {
            models: res.restoredModels,
            combos: res.restoredCombos,
            resources: res.restoredResources,
          })
        );
      } else {
        message(res.message || t("settings.backup_restore_failed"));
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      message(msg || t("settings.backup_restore_failed"));
    } finally {
      setRestoring(false);
      if (fileInputRef.current) {
        fileInputRef.current.value = "";
      }
    }
  };

  return (
    <>
      {/* 1. Tam Sistem Yedeklemesi (Dışa Aktar) */}
      <TitledCard title={t("settings.backup_export_title")}>
        <div className={styles.backupCard}>
          <div className={styles.backupHeader}>
            <small>{t("settings.backup_export_desc")}</small>
          </div>

          <div className={styles.scopePills}>
            <span className={styles.scopePill}>✓ {t("settings.backup_scope_accounts")}</span>
            <span className={styles.scopePill}>✓ {t("settings.backup_scope_models")}</span>
            <span className={styles.scopePill}>✓ {t("settings.backup_scope_combos")}</span>
            <span className={styles.scopePill}>✓ {t("settings.backup_scope_settings")}</span>
          </div>

          <span className={styles.backupNote}>{t("settings.backup_note")}</span>

          <div className={styles.backupActions}>
            <Button size="medium" disabled={exporting} onClick={handleExport}>
              {exporting ? t("settings.backup_exporting") : t("settings.backup_export_btn")}
            </Button>
          </div>
        </div>
      </TitledCard>

      {/* 2. Yedekten Geri Yükle (İçe Aktar) */}
      <TitledCard title={t("settings.backup_restore_title")}>
        <div className={styles.backupCard}>
          <div className={styles.backupHeader}>
            <small>{t("settings.backup_restore_desc")}</small>
          </div>

          <input
            type="file"
            ref={fileInputRef}
            accept=".json"
            style={{ display: "none" }}
            onChange={handleFileChange}
          />

          <div className={styles.backupActions}>
            <Button
              size="medium"
              variant="secondary"
              disabled={restoring}
              onClick={() => fileInputRef.current?.click()}
            >
              {restoring ? t("settings.backup_restoring") : t("settings.backup_restore_btn")}
            </Button>
          </div>
        </div>
      </TitledCard>
    </>
  );
}
