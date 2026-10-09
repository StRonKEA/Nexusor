import { getVersion } from "@tauri-apps/api/app";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { api, type DesktopSettings } from "../api";

export function hasNativeAppLifecycle(): boolean {
  return isTauri();
}

export async function currentAppVersion(): Promise<string> {
  return hasNativeAppLifecycle() ? getVersion() : "dev";
}

export async function readAutostart(): Promise<boolean> {
  return isEnabled();
}

export async function writeAutostart(enabled: boolean): Promise<void> {
  await (enabled ? enable() : disable());
}

export async function isHeadlessServiceInstalled(): Promise<boolean> {
  if (!hasNativeAppLifecycle()) return false;
  return invoke<boolean>("is_headless_service_installed").catch(() => false);
}

export async function setHeadlessService(enabled: boolean): Promise<void> {
  if (!hasNativeAppLifecycle()) return;
  await invoke("set_headless_service", { enabled });
}

export async function readDesktopSettings(): Promise<DesktopSettings> {
  return api.desktopSettings();
}

export async function writeSilentStart(silentStart: boolean): Promise<void> {
  const settings = await readDesktopSettings();
  await api.setDesktopSettings({ ...settings, silent_start: silentStart });
}

export async function writeHideCursorBuiltinModels(hide: boolean): Promise<void> {
  const settings = await readDesktopSettings();
  await api.setDesktopSettings({ ...settings, hide_cursor_builtin_models: hide });
}
