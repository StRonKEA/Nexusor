import anthropicSvg from "../../assets/providers/anthropic.svg";
import antigravitySvg from "../../assets/providers/antigravity.svg";
import codexSvg from "../../assets/providers/codex.svg";
import deepseekSvg from "../../assets/providers/deepseek.svg";
import grokSvg from "../../assets/providers/grok.svg";
import ollamaSvg from "../../assets/providers/ollama.svg";
import openaiSvg from "../../assets/providers/openai.svg";
import openrouterSvg from "../../assets/providers/openrouter.svg";
import copilotSvg from "../../assets/providers/copilot.svg";
import kimiSvg from "../../assets/providers/kimi.svg";
import claudeCodeSvg from "../../assets/providers/claude-code.svg";
import groqSvg from "../../assets/providers/groq.svg";
import nvidiaSvg from "../../assets/providers/nvidia.svg";
import opencodeSvg from "../../assets/providers/opencode.svg";

export const providerLogos: Record<string, string> = {
  openai: openaiSvg,
  anthropic: anthropicSvg,
  deepseek: deepseekSvg,
  openrouter: openrouterSvg,
  ollama: ollamaSvg,
  grok: grokSvg,
  antigravity: antigravitySvg,
  codex: codexSvg,
  copilot: copilotSvg,
  github: copilotSvg,
  kimi: kimiSvg,
  moonshot: kimiSvg,
  "claude-code": claudeCodeSvg,
  claude_code: claudeCodeSvg,
  claudecode: claudeCodeSvg,
  groq: groqSvg,
  nvidia: nvidiaSvg,
  "nvidia-nim": nvidiaSvg,
  opencode: opencodeSvg,
};

export function getProviderLogo(keyOrName?: string | null): string | null {
  if (!keyOrName) return null;
  const lower = keyOrName.toLowerCase();
  if (lower.includes("nvidia")) return nvidiaSvg;
  if (lower.includes("opencode")) return opencodeSvg;
  if (lower.includes("groq")) return groqSvg;
  if (lower.includes("copilot") || lower.includes("github")) return copilotSvg;
  if (lower.includes("kimi") || lower.includes("moonshot")) return kimiSvg;
  if (lower.includes("claude-code") || lower.includes("claudecode") || lower.includes("claude code")) return claudeCodeSvg;
  if (lower.includes("deepseek")) return deepseekSvg;
  if (lower.includes("openrouter")) return openrouterSvg;
  if (lower.includes("ollama")) return ollamaSvg;
  if (lower.includes("grok") || lower.includes("xai") || lower.includes("x.ai")) return grokSvg;
  if (lower.includes("antigravity") || lower.includes("google") || lower.includes("gemini")) return antigravitySvg;
  if (lower.includes("codex") || lower.includes("chatgpt")) return codexSvg;
  if (lower.includes("claude") || lower.includes("anthropic")) return anthropicSvg;
  if (lower.includes("openai") || lower.includes("gpt")) return openaiSvg;
  return null;
}
