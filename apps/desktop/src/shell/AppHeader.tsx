import { useEffect, useState } from "react";
import appIcon from "../assets/app-icon.png";
import { currentAppVersion } from "../shared/native/appLifecycle";
import type { DesktopPlatform } from "../shared/native/platform";
import { WindowControls } from "./WindowControls";
import styles from "./AppHeader.module.scss";

type AppHeaderProps = {
  platform: DesktopPlatform;
  nativeDesktop: boolean;
};

export function AppHeader({ nativeDesktop }: AppHeaderProps) {
  const [version, setVersion] = useState("…");

  useEffect(() => {
    let disposed = false;
    void currentAppVersion().then((next) => {
      if (!disposed) setVersion(next);
    });
    return () => { disposed = true; };
  }, []);

  return <header className={styles.root}>
    <div className={styles.dragLayer} data-tauri-drag-region aria-hidden="true" />
    <div className={styles.uiLayer}>
      {nativeDesktop && <>
        <div className={styles.identity} aria-label="Nexusor">
          <img src={appIcon} alt="" aria-hidden="true" />
          <span>Nexusor v{version}</span>
        </div>
        <WindowControls />
      </>}
    </div>
  </header>;
}
