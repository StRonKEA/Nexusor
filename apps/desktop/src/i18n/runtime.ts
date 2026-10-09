import enUS from "./locales/en-US.json";
import trTR from "./locales/tr-TR.json";
import zhCN from "./locales/zh-CN.json";
import ptBR from "./locales/pt-BR.json";

export type Locale = "en-US" | "tr-TR" | "zh-CN" | "pt-BR";
export type CommitPromptLocale = "en-US" | "zh-CN";

export function commitPromptLocale(locale: Locale): CommitPromptLocale {
  return locale === "zh-CN" ? "zh-CN" : "en-US";
}

export type TranslationValue = string | number;
export type TranslationParams = Readonly<Record<string, TranslationValue>>;

const localeMessages: Record<Locale, Record<string, string>> = {
  "en-US": enUS as Record<string, string>,
  "tr-TR": trTR as Record<string, string>,
  "zh-CN": zhCN as Record<string, string>,
  "pt-BR": ptBR as Record<string, string>,
};

let currentMessages: Record<string, string> = localeMessages["en-US"];

export function setRuntimeLocale(locale: Locale) {
  currentMessages = localeMessages[locale] || localeMessages["en-US"];
}

export function t(key: string, params?: TranslationParams): string {
  const template = currentMessages[key] || (enUS as Record<string, string>)[key] || key;
  if (!params) return template;
  return template.replace(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g, (_match, name: string) => {
    if (!Object.prototype.hasOwnProperty.call(params, name)) {
      return `{${name}}`;
    }
    return String(params[name]);
  });
}
