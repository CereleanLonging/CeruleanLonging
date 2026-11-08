import type { ApiTarget, ChiefSource, DeputySource, ReasoningEffort } from "./contracts";
import { managementEndpoints } from "./ManagementProviders";
import { preferenceKey, readPreference } from "./accessPreferences";

export type ApiModelAssignment = {
  id: string;
  label: string;
  target: ApiTarget;
  model: string;
  reasoningEffort?: ReasoningEffort | null;
  onSelect: (model: string, target: ApiTarget, reasoningEffort?: ReasoningEffort) => void;
};

export function apiTargetKey(target: ApiTarget): string {
  if (target.kind === "openAi") return "openAi";
  const endpoint = target.endpoint;
  return JSON.stringify([endpoint.kind, endpoint.kind === "qwenBeijing" ? endpoint.workspaceId : null]);
}

export function managementApiTarget(source: ChiefSource | DeputySource | undefined): ApiTarget | undefined {
  if (source?.kind === "codexSession" || source?.kind === "omp") return undefined;
  if (source?.kind === "deepSeek") return { kind: "provider", endpoint: { kind: "deepSeek" } };
  return source ?? { kind: "openAi" };
}

const familyAliases: [string, string[]][] = [
  ["deepseek", ["ds", "深度求索"]],
  ["qwen", ["qw", "通义千问", "千问", "通义"]],
  ["claude", ["克劳德"]],
  ["gemini", ["双子星"]],
  ["kimi", ["月之暗面"]],
  ["chatgpt", ["gpt"]],
];

function compact(value: string): string {
  return value.normalize("NFKC").toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");
}

export function matchesApiModel(model: string, query: string): boolean {
  const normalized = model.normalize("NFKC").toLowerCase();
  const alternatives = [compact(model)];
  for (const [family, aliases] of familyAliases) {
    if (normalized.includes(family)) {
      alternatives.push(...aliases.map(alias => compact(normalized.replaceAll(family, alias))));
    }
  }
  const words = normalized.match(/[a-z]+|[0-9]+/g) ?? [];
  alternatives.push(words.map(word => /^\d/.test(word) ? word : word[0]).join(""));
  const terms = query.normalize("NFKC").trim().split(/\s+/).map(compact).filter(Boolean);
  return terms.every(term => alternatives.some(candidate => {
    if (candidate.includes(term)) return true;
    const parts = term.match(/[\p{L}]+|[\p{N}]+/gu) ?? [];
    return parts.length > 1 && parts.every(part => candidate.includes(part));
  }));
}

export function parseApiModelDefault(value: string | undefined): { target: ApiTarget; model: string; reasoningEffort?: ReasoningEffort } | undefined {
  if (!value) return;
  try {
    const item = JSON.parse(value);
    if (!item || typeof item.model !== "string" || !item.model.trim() || item.model.length > 512) return;
    const reasoningEffort = item.reasoningEffort;
    if (reasoningEffort != null && !["none","minimal","low","medium","high","xhigh","max","ultra"].includes(reasoningEffort)) return;
    if (item.target?.kind === "openAi") return { target: { kind: "openAi" }, model: item.model, reasoningEffort };
    const endpoint = item.target?.endpoint;
    if (item.target?.kind !== "provider" || !managementEndpoints.some(entry => entry.kind === endpoint?.kind)) return;
    if (endpoint.kind === "qwenBeijing") {
      if (typeof endpoint.workspaceId !== "string" || !/^[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$/.test(endpoint.workspaceId)) return;
      return { target: { kind: "provider", endpoint: { kind: "qwenBeijing", workspaceId: endpoint.workspaceId } }, model: item.model, reasoningEffort };
    }
    return { target: { kind: "provider", endpoint: { kind: endpoint.kind } }, model: item.model, reasoningEffort };
  } catch { return; }
}

export function readApiModelDefault(scope: string, role: "chief" | "deputy" | "worker") {
  return parseApiModelDefault(readPreference(preferenceKey(scope, `model-default:${role}`)));
}
