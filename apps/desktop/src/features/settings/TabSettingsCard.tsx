import type { TabMode, TabSettings } from "../../shared/api";
import { Button } from "../../shared/ui/Button";
import { TextInput } from "../../shared/ui/FormControls";
import { Select } from "../../shared/ui/Select";
import { TitledCard } from "../../shared/ui/TitledCard";
import styles from "./TabSettingsCard.module.scss";

export function TabSettingsCard({
  settings,
  draft,
  editing,
  saving,
  onDraftChange,
  onEdit,
  onCancel,
  onSave,
}: {
  settings: TabSettings | null;
  draft: TabSettings;
  editing: boolean;
  saving: boolean;
  onDraftChange: (settings: TabSettings) => void;
  onEdit: () => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  const modeLabel = (mode: TabMode) => {
    if (mode === "public") return t("settings.use_public_service");
    if (mode === "direct") return t("settings.direct");
    return t("home.custom");
  };
  const action = editing ? (
    <div className={styles.actionGroup}>
      <Button size="small" disabled={saving} onClick={onCancel}>{t("common.cancel")}</Button>
      <Button variant="primary" size="small" disabled={saving} onClick={onSave}>{saving ? t("settings.saving") : t("common.save")}</Button>
    </div>
  ) : (
    <button type="button" className={styles.headerAction} disabled={!settings} onClick={onEdit}>{t("common.edit")}</button>
  );

  return <TitledCard
    title={<div className={styles.title}><span>{t("settings.tab_settings")}</span></div>}
    action={action}
  >
    <div className={styles.content}>
      {editing ? <>
        <div className={styles.row}>
          <div className={styles.description}>
            <strong>{t("settings.tab_connection")}</strong>
            <small>{t("settings.choose_how_cursor_connects_to_ta")}</small>
          </div>
          <div className={styles.control}><Select
            value={draft.mode}
            ariaLabel={t("settings.tab_connection")}
            options={[
              { value: "public", label: t("settings.use_public_service") },
              { value: "direct", label: t("settings.direct") },
              { value: "custom", label: t("home.custom") },
            ]}
            onChange={(mode) => onDraftChange({ ...draft, mode: mode as TabMode })}
          /></div>
        </div>
        {draft.mode === "custom" && <div className={styles.row}>
          <div className={styles.description}>
            <strong>{t("settings.tab_service_address")}</strong>
            <small>{t("settings.the_original_endpoint_path_is_ap")}</small>
          </div>
          <div className={styles.control}><TextInput
            value={draft.address}
            placeholder="https://tab.example.com"
            aria-label={t("settings.tab_service_address")}
            onChange={(event) => onDraftChange({ ...draft, address: event.target.value })}
            onKeyDown={(event) => { if (event.key === "Enter") onSave(); }}
          /></div>
        </div>}
      </> : <>
        <div className={styles.row}>
          <strong>{t("settings.tab_connection")}</strong>
          <span className={styles.value}>{settings ? modeLabel(settings.mode) : t("settings.loading")}</span>
        </div>
        {settings?.mode === "custom" && <div className={styles.row}>
          <strong>{t("settings.tab_service_address")}</strong>
          <span className={styles.value}>{settings.address}</span>
        </div>}
      </>}
    </div>
  </TitledCard>;
}
