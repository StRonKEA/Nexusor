import { createContext, useContext, type ReactNode } from "react";
import { useAppStore } from "../../shared/store/appStore";
import controls from "../../shared/ui/Controls.module.scss";
import styles from "./CursorSettings.module.scss";

const CaReady = createContext(false);
const ModelsReady = createContext(false);

export function CursorCaProvider({ children }: { children: ReactNode }) {
  const { cursorHarness } = useAppStore();
  return <CaReady.Provider value={cursorHarness?.ca === "ready"}>{children}</CaReady.Provider>;
}

export function CursorCaGate({ busy, waitingForRefresh, onInitialize, onRefresh, children }: { busy: boolean; waitingForRefresh: boolean; onInitialize: () => void; onRefresh: () => void; children: ReactNode }) {
  const ready = useContext(CaReady);
  const { cursorHarness } = useAppStore();
  if (ready) return children;
  const installedLocally = cursorHarness?.ca === "untrusted";
  return <div className={styles.gate}>
    <strong>{installedLocally ? t("models.the_local_ca_must_be_trusted_by_") : t("models.initialize_the_local_ca_first")}</strong>
    <span>{installedLocally ? t("models.paste_the_authorization_command_") : t("models.the_ca_is_stored_only_on_this_de")}</span>
    <button className={controls.primary} disabled={busy} onClick={waitingForRefresh ? onRefresh : onInitialize}>{busy ? t("models.refreshing") : waitingForRefresh ? t("models.i_ve_initialized_it_refresh") : installedLocally ? t("models.open_terminal_to_install_ca") : t("models.initialize_ca")}</button>
  </div>;
}

export function CursorModelProvider({ children }: { children: ReactNode }) {
  const { models, plugins } = useAppStore();
  const hasConfiguredPlugin = plugins.some((plugin) => plugin.providers.some((provider) => provider.configured));
  return <ModelsReady.Provider value={models.length > 0 || hasConfiguredPlugin}>{children}</ModelsReady.Provider>;
}

export function CursorModelGate({ busy, onNavigateToProviders, children }: { busy: boolean; onNavigateToProviders: () => void; children: ReactNode }) {
  const ready = useContext(ModelsReady);
  if (ready) return children;
  return <div className={styles.gate}>
    <strong>{t("models.no_models_are_available_to_curso")}</strong>
    <span>{t("models.no_model_provider_connected_plea")}</span>
    <div className={styles.gateActions}>
      <button className={controls.primary} disabled={busy} onClick={onNavigateToProviders}>{t("models.go_to_providers")}</button>
    </div>
  </div>;
}
