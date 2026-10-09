import { useState } from "react";
import type { IconifyIcon } from "@iconify/react/offline";
import KeepAliveRouteOutlet from "keepalive-for-react-router";
import { NavLink, useLocation } from "react-router-dom";
import { PageLayout } from "./layout/PageLayout";
import { Card } from "../shared/ui/Card";
import controls from "../shared/ui/Controls.module.scss";
import { Icon } from "../shared/ui/Icon";
import { TooltipTrigger } from "../shared/ui/TooltipTrigger";
import {
  activityIcon,
  combosIcon,
  cursorIcon,
  homeIcon,
  modelsIcon,
  providerIcon,
  refreshIcon,
  settingsIcon,
} from "../shared/ui/icons";
import { appStore, useAppStore } from "../shared/store/appStore";
import styles from "./AppLayout.module.scss";
import { PageActionsTarget } from "./PageActions";

type MenuItem =
  | { kind: "page"; path: string; label: string; icon: IconifyIcon | string; end?: boolean }
  | { kind: "group"; label: string };

const keptAlivePages = ["/", "/calls", "/settings", "/cursor", "/providers", "/models", "/combos"];

export function AppLayout() {
  const { busy, cursorHarness } = useAppStore();
  const location = useLocation();
  const [leftActionTarget, setLeftActionTarget] = useState<HTMLDivElement | null>(null);
  const [rightActionTarget, setRightActionTarget] = useState<HTMLDivElement | null>(null);

  const menuItems: MenuItem[] = [
    { kind: "group", label: t("nav.general") },
    { kind: "page", path: "/", label: t("nav.overview"), icon: homeIcon, end: true },
    { kind: "page", path: "/cursor", label: t("nav.cursor_integration"), icon: cursorIcon },
    { kind: "group", label: t("nav.resources") },
    { kind: "page", path: "/providers", label: t("nav.providers"), icon: providerIcon },
    { kind: "page", path: "/models", label: t("nav.models"), icon: modelsIcon },
    { kind: "page", path: "/combos", label: t("nav.routing"), icon: combosIcon },
    { kind: "group", label: t("nav.observability") },
    { kind: "page", path: "/calls", label: t("nav.calls"), icon: activityIcon },
    { kind: "group", label: t("nav.system") },
    { kind: "page", path: "/settings", label: t("nav.settings"), icon: settingsIcon },
  ];

  const cacheKey = location.pathname;

  return <PageLayout className={styles.root}>
    <Card as="aside" className={styles.menuCard}>
      <nav className={styles.navigation} aria-label={t("nav.main_menu")}>
        <div className={styles.navigationList}>
          {menuItems.map((item) =>
            item.kind === "group" ? (
              <div className={styles.navigationGroup} key={`group-${item.label}`}>
                {item.label}
              </div>
            ) : (
              <div className={styles.navigationRow} key={item.path}>
                <NavLink to={item.path} end={item.end ?? item.path === "/"}>
                  {typeof item.icon === "string" ? (
                    <Icon src={item.icon} size="1.3em" />
                  ) : (
                    <Icon icon={item.icon} size="1.3em" />
                  )}
                  <span>{item.label}</span>
                  {item.path === "/cursor" && cursorHarness && (
                    <span
                      className={styles.menuStatusTag}
                      data-taken={cursorHarness.settings_applied || undefined}
                    >
                      {cursorHarness.settings_applied
                        ? t("cursor.taken_over")
                        : t("cursor.not_taken_over")}
                    </span>
                  )}
                </NavLink>
              </div>
            )
          )}
        </div>
      </nav>
    </Card>
    <main className={styles.content}>
      <div className={styles.actionRegion}>
        <Card className={styles.actions}>
          <div ref={setLeftActionTarget} className={styles.pageActions} />
          <TooltipTrigger label={t("common.refresh")}>
            <button
              className={controls.iconButton}
              aria-label={t("common.refresh")}
              disabled={busy}
              onClick={() => void appStore.refresh()}
            >
              <Icon className={busy ? controls.spin : ""} icon={refreshIcon} size="1.1em" />
            </button>
          </TooltipTrigger>
          <div ref={setRightActionTarget} className={styles.pageActions} />
        </Card>
      </div>
      <PageActionsTarget.Provider value={{ left: leftActionTarget, right: rightActionTarget }}>
        <KeepAliveRouteOutlet
          activeCacheKey={cacheKey}
          include={keptAlivePages}
        />
      </PageActionsTarget.Provider>
    </main>
  </PageLayout>;
}
