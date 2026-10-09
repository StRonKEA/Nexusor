import { useEffect, useState } from "react";
import {
  currentAppVersion,
  hasNativeAppLifecycle,
  isHeadlessServiceInstalled,
  readAutostart,
  setHeadlessService,
  writeAutostart,
} from "../../shared/native/appLifecycle";
import { Switch } from "../../shared/ui/Switch";
import { TitledCard } from "../../shared/ui/TitledCard";
import { useMessage } from "../../shared/ui/message";
import styles from "./AppLifecycleSettingsCard.module.scss";

export function AppLifecycleSettingsCard() {
  const message = useMessage();
  const native = hasNativeAppLifecycle();
  const [version, setVersion] = useState("…");
  const [autostart, setAutostart] = useState(false);
  const [loadingAutostart, setLoadingAutostart] = useState(native);
  const [headlessService, setHeadlessServiceState] = useState(false);
  const [loadingHeadless, setLoadingHeadless] = useState(native);

  useEffect(() => {
    let disposed = false;
    void currentAppVersion().then((next) => { if (!disposed) setVersion(next); });
    if (native) {
      void readAutostart()
        .then((enabled) => { if (!disposed) setAutostart(enabled); })
        .catch((cause) => message(cause instanceof Error ? cause.message : String(cause)))
        .finally(() => { if (!disposed) setLoadingAutostart(false); });
      void isHeadlessServiceInstalled()
        .then((installed) => { if (!disposed) setHeadlessServiceState(installed); })
        .catch(() => {})
        .finally(() => { if (!disposed) setLoadingHeadless(false); });
    }
    return () => { disposed = true; };
  }, [message, native]);

  const toggleAutostart = async (enabled: boolean) => {
    try {
      setLoadingAutostart(true);
      if (enabled && headlessService) {
        // İkisi aynı anda açık olamaz: Headless servisi kapat
        await setHeadlessService(false);
        setHeadlessServiceState(false);
      }
      await writeAutostart(enabled);
      setAutostart(await readAutostart());
      message(enabled ? "Başlangıçta sistem tepsisinde açılma etkinleştirildi" : "Başlangıçta açılma kapatıldı");
    } catch (cause) {
      message(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoadingAutostart(false);
    }
  };

  const toggleHeadlessService = async (enabled: boolean) => {
    try {
      setLoadingHeadless(true);
      if (enabled && autostart) {
        // İkisi aynı anda açık olamaz: Sistem tepsisinde başlatmayı kapat
        await writeAutostart(false);
        setAutostart(false);
      }
      await setHeadlessService(enabled);
      setHeadlessServiceState(await isHeadlessServiceInstalled());
      message(enabled ? "Görünmez arka plan servisi kuruldu (Sistem tepsisi başlangıcı kapatıldı)" : "Arka plan servisi kaldırıldı");
    } catch (cause) {
      message(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoadingHeadless(false);
    }
  };

  return <TitledCard title={t("settings.application")}>
    <div className={styles.row}>
      <div>
        <strong>{t("settings.current_version")}</strong>
        <small>v{version}</small>
      </div>
    </div>
    <div className={styles.row}>
      <div>
        <strong>Windows Açılışında Sistem Tepsisinde Başlat</strong>
        <small>Bilgisayar açıldığında Nexusor penceresiz olarak saatin yanında hazır beklesin.</small>
      </div>
      <Switch
        checked={autostart}
        disabled={!native || loadingAutostart}
        label="Sistem Tepsisinde Başlat"
        onChange={(enabled) => void toggleAutostart(enabled)}
      />
    </div>
    <div className={styles.row}>
      <div>
        <strong>Görünmez Arka Plan Servisi (Headless Daemon)</strong>
        <small>Hiçbir arayüz ve simge olmadan, yalnızca arka plan proxy motoru otomatik çalışsın (Sıfır RAM/UI yükü).</small>
      </div>
      <Switch
        checked={headlessService}
        disabled={!native || loadingHeadless}
        label="Görünmez Servis Olarak Başlat"
        onChange={(enabled) => void toggleHeadlessService(enabled)}
      />
    </div>
  </TitledCard>;
}
