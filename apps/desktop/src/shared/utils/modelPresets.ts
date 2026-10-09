import type { ModelType } from "../api";
import { defaultCustomHeaders } from "./modelDefaults";
import { providerLogos } from "./providerIcons";

export interface ModelPresetEntry {
  model_id: string;
  display_name: string;
  context_window_tokens: number | null;
  max_output_tokens: number | null;
}

/** 一个服务商在某种协议（anthropic / openai）下的接入端点 */
export interface ModelPresetEndpoint {
  baseUrl: string;
  /** true 时 baseUrl 即完整请求 URL；false 时由请求协议追加标准端点路径 */
  useFullUrl: boolean;
  /** openai 协议的请求端点（useFullUrl 为 true 时忽略） */
  openaiEndpoint: string;
  /** 非空时启用自定义 Headers（claude-cli 伪装头） */
  customHeaders: Record<string, string> | null;
}

export interface ModelPreset {
  key: string;
  name: string;
  icon: string;
  keyHint: string;
  /** 五家服务商均同时提供 Anthropic 与 OpenAI 兼容协议 */
  endpoints: { anthropic: ModelPresetEndpoint; openai: ModelPresetEndpoint };
  models: ModelPresetEntry[];
}

const claudeHeaders = { ...defaultCustomHeaders };
/** anthropic: Base URL auto-appends /v1/messages */
const anthropic = (baseUrl: string): ModelPresetEndpoint => ({ baseUrl, useFullUrl: false, openaiEndpoint: "", customHeaders: claudeHeaders });
/** openai: Base URL auto-appends /v1/chat/completions */
const openaiChat = (baseUrl: string): ModelPresetEndpoint => ({ baseUrl, useFullUrl: false, openaiEndpoint: "/v1/chat/completions", customHeaders: null });

export const modelPresets: ModelPreset[] = [
  {
    key: "openai",
    name: "OpenAI",
    icon: providerLogos.openai,
    keyHint: "api.openai.com",
    endpoints: {
      anthropic: anthropic("https://api.openai.com"),
      openai: openaiChat("https://api.openai.com"),
    },
    models: [],
  },
  {
    key: "anthropic",
    name: "Anthropic",
    icon: providerLogos.anthropic,
    keyHint: "api.anthropic.com",
    endpoints: {
      anthropic: anthropic("https://api.anthropic.com"),
      openai: openaiChat("https://api.anthropic.com"),
    },
    models: [],
  },
  {
    key: "openrouter",
    name: "OpenRouter",
    icon: providerLogos.openrouter,
    keyHint: "openrouter.ai/api/v1",
    endpoints: {
      anthropic: anthropic("https://openrouter.ai/api/v1"),
      openai: openaiChat("https://openrouter.ai/api/v1"),
    },
    models: [],
  },
  {
    key: "deepseek",
    name: "DeepSeek",
    icon: providerLogos.deepseek,
    keyHint: "api.deepseek.com",
    endpoints: {
      anthropic: anthropic("https://api.deepseek.com"),
      openai: openaiChat("https://api.deepseek.com"),
    },
    models: [],
  },
  {
    key: "ollama",
    name: "Ollama / Local",
    icon: providerLogos.ollama,
    keyHint: "localhost:11434",
    endpoints: {
      anthropic: anthropic("http://localhost:11434"),
      openai: openaiChat("http://localhost:11434"),
    },
    models: [],
  },
];

export const trimTrailingSlash = (url: string) => url.replace(/\/+$/, "");

export const presetEndpoint = (preset: ModelPreset, type: ModelType): ModelPresetEndpoint => preset.endpoints[type];
