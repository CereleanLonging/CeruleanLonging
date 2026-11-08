import type { ChiefSource, DeputySource, ProviderEndpoint, Snapshot } from "./contracts";

export const managementEndpoints: {kind: ProviderEndpoint["kind"]; label: string}[] = [
  {kind:"a6Api",label:"A6API 中转 · api.a6api.com"},
  {kind:"a6ApiVip",label:"A6API 中转 · api.a6api.vip"},
  {kind:"deepSeek",label:"DeepSeek API"},
  {kind:"anthropic",label:"Claude API"},
  {kind:"kimiChina",label:"Kimi 中国站"},
  {kind:"grok",label:"xAI Grok API"},
  {kind:"qwenBeijing",label:"Qwen 北京 API"},
  {kind:"qwenSingapore",label:"Qwen 新加坡 API"},
  {kind:"qwenHongKong",label:"Qwen 中国香港 API"},
];
export function endpointProvider(endpoint: ProviderEndpoint) {
  return ({a6Api:"a6api",a6ApiVip:"a6api",deepSeek:"deepseek",anthropic:"anthropic",kimiChina:"kimi",grok:"grok",qwenBeijing:"qwen",qwenSingapore:"qwen",qwenHongKong:"qwen"} as const)[endpoint.kind];
}
export function managementKeyMissing(data: Snapshot, source: ChiefSource | DeputySource | undefined): boolean {
  if(source?.kind === "codexSession") return false;
  if(!source || source.kind === "openAi") return !data.openaiConnection.keyConfigured;
  const provider = (source.kind === "deepSeek" || source.kind === "omp") ? "deepseek" : endpointProvider(source.endpoint);
  return !data.providerCredentials?.some(c=>c.provider === provider && c.keyConfigured);
}
export function managementModelList(data: Snapshot, source: ChiefSource | DeputySource | undefined): string[] {
  if(source?.kind === "codexSession") return [];
  if(!source || source.kind === "openAi") return data.openaiConnection.models;
  const endpoint: ProviderEndpoint = (source.kind === "deepSeek" || source.kind === "omp") ? {kind:"deepSeek"} : source.endpoint;
  return data.providerConnections?.find(c=>c.provider === endpointProvider(endpoint)
    && c.target?.kind === endpoint.kind
    && (endpoint.kind !== "qwenBeijing" || c.target.kind === "qwenBeijing" && c.target.workspaceId === endpoint.workspaceId))?.models ?? [];
}
