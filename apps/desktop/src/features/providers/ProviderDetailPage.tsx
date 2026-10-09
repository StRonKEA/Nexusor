import { Link, useParams, useSearchParams } from "react-router-dom";
import { useEffect, useRef, useState } from "react";
import {
  api,
  pluginText,
  type PluginDescriptor,
  type PluginImportFile,
} from "../../shared/api";
import { useI18n } from "../../i18n/store";
import { PageContent } from "../../shell/layout/PageContent";
import { PageActions } from "../../shell/PageActions";
import { appStore, useAppStore } from "../../shared/store/appStore";
import { Button } from "../../shared/ui/Button";
import { Card } from "../../shared/ui/Card";
import { getProviderLogo } from "../../shared/utils/providerIcons";
import { PluginAddPanel, PluginSettingsPanel } from "../plugins/PluginResourcePanels";
import styles from "./ProviderDetailPage.module.scss";

export function ProviderDetailPage() {
  const { pluginId = "" } = useParams();
  const [searchParams] = useSearchParams();
  const { plugins, ports } = useAppStore();
  const { locale } = useI18n();
  const plugin = plugins.find((item) => item.id === pluginId) ?? null;
  const hasAccounts = plugin ? plugin.resources.some((r) => r.resources.length > 0) : false;
  const initialSection = searchParams.get("tab") === "accounts" && hasAccounts ? "accounts" : "add";
  const [section, setSection] = useState<"accounts" | "add">(initialSection);

  useEffect(() => {
    if (!hasAccounts) {
      setSection("add");
    }
  }, [hasAccounts]);

  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  if (!plugin) {
    return (
      <PageContent
        title={t("calls.providers")}
        sections={[{
          key: "missing",
          estimatedHeight: 240,
          content: <div className={styles.empty}>
            <strong>{t("providers.no_plugins_installed")}</strong>
            <Link to="/providers">{t("providers.back_to_providers")}</Link>
          </div>,
        }]}
      />
    );
  }

  const importable = plugin.resources.find((resource) => resource.import);
  const exportable = plugin.resources[0];

  const onImportFiles = async (files: FileList | null) => {
    if (!files?.length || !importable?.import) return;
    setBusy(true);
    setMessage(null);
    try {
      const payload: PluginImportFile[] = [];
      for (const file of Array.from(files)) {
        payload.push({
          name: file.name,
          content: await file.text(),
        });
      }
      const result = await api.importPluginResources(plugin.id, importable.type, payload);
      await appStore.refreshPlugins();
      setMessage(t("providers.import_finished_added_updated", {
        added: result.added,
        updated: result.updated,
      }));
      if (result.modelSyncError) {
        setMessage(t("plugins.the_account_was_saved_but_model_", { error: result.modelSyncError }));
      }
      setSection("accounts");
    } catch (cause) {
      setMessage(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
      if (fileInput.current) fileInput.current.value = "";
    }
  };

  const onExport = () => {
    if (!exportable) return;
    const url = api.pluginResourceExportUrl(ports.service_port, plugin.id, exportable.type);
    void api.openExternalUrl(url);
  };

  const help = oauthHelp(plugin);

  return (
    <>
      <PageActions position="left">
        <Link className={styles.backLink} to="/providers">{t("providers.back_to_providers")}</Link>
      </PageActions>
      <PageActions>
        <div className={styles.toolbar}>
          <Button size="small" variant={section === "add" ? "primary" : undefined} onClick={() => setSection("add")}>
            {t("providers.add_account")}
          </Button>
          {hasAccounts && (
            <Button size="small" variant={section === "accounts" ? "primary" : undefined} onClick={() => setSection("accounts")}>
              {t("providers.manage_accounts")}
            </Button>
          )}
          {importable?.import && <>
            <input
              ref={fileInput}
              type="file"
              hidden
              multiple={importable.import.multiple}
              accept={importable.import.accept.join(",")}
              onChange={(event) => void onImportFiles(event.target.files)}
            />
            <Button size="small" disabled={busy} onClick={() => fileInput.current?.click()}>
              {t("providers.import_accounts")}
            </Button>
          </>}
          {exportable && hasAccounts && <Button size="small" disabled={busy} onClick={onExport}>{t("providers.export_accounts")}</Button>}
        </div>
      </PageActions>
      <PageContent
        title={plugin.name}
        sections={[{
          key: "detail",
          estimatedHeight: 720,
          content: <div className={styles.page}>
            <Card className={styles.headerCard}>
              {(() => {
                const logo = getProviderLogo(plugin.id) || getProviderLogo(plugin.name);
                return logo ? (
                  <img className={styles.icon} src={logo} alt="" style={{ width: "36px", height: "36px", objectFit: "contain", borderRadius: "6px" }} />
                ) : null;
              })()}
              <div className={styles.headerText}>
                <strong>{plugin.name}</strong>
                <span>
                  {plugin.providers
                    .map((provider) => pluginText(provider.displayName, locale))
                    .join(" · ")}
                </span>
                {help && <small>{help}</small>}
              </div>
            </Card>
            {message && <div className={styles.banner} role="status">{message}</div>}
            {section === "add"
              ? <PluginAddPanel plugin={plugin} onConfigured={() => setSection("accounts")} />
              : <PluginSettingsPanel plugin={plugin} />}
          </div>,
        }]}
      />
    </>
  );
}

function oauthHelp(plugin: PluginDescriptor): string | null {
  const methods = plugin.resources.flatMap((resource) => resource.add);
  if (methods.some((method) => method.type === "oauth2.authorization-code")) {
    return t("providers.this_provider_uses_browser_autho");
  }
  if (methods.some((method) => method.type === "oauth2.0")) {
    return t("providers.this_provider_uses_device_code_l");
  }
  return null;
}
