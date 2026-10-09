import { useEffect, useMemo, useState } from "react";
import { api, pluginText, type CmdKSettings } from "../../shared/api";
import { useI18n } from "../../i18n/store";
import { useAppStore } from "../../shared/store/appStore";
import { Button } from "../../shared/ui/Button";
import { ModelSelect, type ModelSelectOption } from "../../shared/ui/ModelSelect";
import { TitledCard } from "../../shared/ui/TitledCard";
import { useMessage } from "../../shared/ui/message";
import { modelProviderName } from "../../shared/utils/modelProvider";
import styles from "./CommitSettingsCard.module.scss";

export function CmdKSettingsCard() {
  const { models, plugins } = useAppStore();
  const { locale } = useI18n();
  const message = useMessage();
  const [saved, setSaved] = useState<CmdKSettings | null>(null);
  const [draft, setDraft] = useState<CmdKSettings | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    let active = true;
    void api.cmdkSettings().then((value) => {
      if (active) { setSaved(value); setDraft(value); }
    }).catch((error: unknown) => { if (active) message(String(error)); });
    return () => { active = false; };
  }, [message]);
  const options = useMemo(() => {
    const result: ModelSelectOption[] = [{ value: "", label: t("settings.direct"), group: "Cursor" }];
    for (const model of models) result.push({ value: model.model_hash, label: model.display_name || model.model_id, group: modelProviderName(model) });
    for (const plugin of plugins) for (const provider of plugin.providers) {
      if (!provider.configured) continue;
      for (const model of provider.models.filter((item) => item.enabled)) result.push({
        value: model.id, label: model.displayName,
        group: pluginText(provider.displayName, locale) || plugin.name,
      });
    }
    for (const id of [saved?.editor_model_id, saved?.terminal_model_id, saved?.tab_model_id]) {
      if (id && !result.some((option) => option.value === id)) result.push({ value: id, label: id, group: "Cursor" });
    }
    return result;
  }, [models, plugins, locale, saved]);
  const dirty = !!draft && !!saved && (draft.editor_model_id !== saved.editor_model_id || draft.terminal_model_id !== saved.terminal_model_id || draft.tab_model_id !== saved.tab_model_id);
  async function save() {
    if (!draft) return;
    setSaving(true);
    try { const value = await api.setCmdKSettings(draft); setSaved(value); setDraft(value); }
    catch (error) { message(String(error)); }
    finally { setSaving(false); }
  }
  return <TitledCard title="Cursor Ctrl+K / Cmd+K / Tab" action={<div className={styles.actionGroup}>
    <Button size="small" disabled={!dirty || saving} onClick={() => setDraft(saved)}>{t("common.cancel")}</Button>
    <Button size="small" variant="primary" disabled={!dirty || saving} onClick={() => void save()}>{saving ? t("settings.saving") : t("common.save")}</Button>
  </div>}>
    <div className={styles.content}>
      {([['editor_model_id', 'Editor Ctrl+K'], ['terminal_model_id', 'Terminal Ctrl+K'], ['tab_model_id', 'Tab']] as const).map(([key, label]) => <div className={styles.row} key={key}>
        <div className={styles.details}><strong>{label}</strong></div>
        <div className={styles.select}><ModelSelect mode="single" label={label} value={draft?.[key] ?? ""} options={options} disabled={!draft || saving}
          onChange={(value) => setDraft((current) => current ? { ...current, [key]: value } : current)} /></div>
      </div>)}
    </div>
  </TitledCard>;
}
