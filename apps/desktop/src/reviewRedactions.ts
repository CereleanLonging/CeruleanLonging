import type { Redaction, Snapshot } from "./contracts";

const bytes = (value: string) => new TextEncoder().encode(value).length;
const longestFirst = (rules: Redaction[]) => rules.sort((a, b) => bytes(b.literal) - bytes(a.literal) || a.literal.localeCompare(b.literal));

export function reviewRulesKey(data: Snapshot | null, projectId: string): string {
  const project = data?.workflows?.find(w => w.project.id === projectId)?.project;
  return JSON.stringify([data?.dataDirectory, projectId, project?.root]);
}

// Generate exact replacements from saved, non-credential project metadata only.
// The independent verifier applies these to requirements, criteria and source
// before fragmenting; its secret/control-character checks remain mandatory.
export function defaultReviewRedactions(data: Snapshot | null, projectId: string): Redaction[] {
  const project = data?.workflows?.find(w => w.project.id === projectId)?.project;
  if (!project) return [];
  const setup = data?.projectSetups?.find(s => s.setup.projectId === projectId)?.setup;
  const paths = [project.root, setup?.chiefDirectory, setup?.worktree?.poolRoot,
    ...Object.values(setup?.workerDirectories ?? {}).map(d => d && `${project.root.replace(/[\\/]+$/, "")}/${d.relativePath}`),
    setup?.desktopWorker?.projectRoot, ...(setup?.profiles ?? []).map(p => p.desktopWorker?.projectRoot)];
  const rules = new Map<string, Redaction>();
  let index = 0;
  for (const path of paths) {
    if (!path) continue;
    const original = path.replace(/\\/g, "/").replace(/\/+$/, "");
    // Project roots from the Windows core commonly have a \\?\ prefix, while
    // source text usually contains the ordinary path. Match both spellings.
    const slash = original.replace(/^\/\/\?\/UNC\//i, "//").replace(/^\/\/\?\/([a-z]:)/i, "$1");
    if (!slash || /^[a-z]:$/i.test(slash)) continue; // Never replace a whole drive/root.
    const replacement = rules.get(slash)?.replacement ?? `REDACTED_DIR_${String(++index).padStart(2, "0")}`;
    const variants = [slash, original].flatMap(value => {
      const backslash = value.replace(/\//g, "\\");
      return [value, backslash, backslash.replace(/\\/g, "\\\\")];
    });
    for (const literal of new Set(variants)) {
      rules.set(literal, { literal, replacement });
    }
  }
  return longestFirst([...rules.values()]);
}

export function combineReviewRedactions(defaults: Redaction[], custom: string): Redaction[] {
  const rules = new Map(defaults.map(r => [r.literal, r]));
  const seen = new Set<string>();
  for (const [index, line] of custom.split("\n").entries()) {
    if (!line.trim()) continue;
    const pos = line.indexOf("=>");
    const literal = pos < 0 ? "" : line.slice(0, pos).trim();
    const replacement = pos < 0 ? "" : line.slice(pos + 2).trim();
    if (!literal || !replacement) throw Error(`第 ${index + 1} 行请写成：原文 => 替代标记`);
    if (seen.has(literal)) throw Error(`第 ${index + 1} 行原文重复，请合并为一条规则`);
    seen.add(literal);
    rules.set(literal, { literal, replacement }); // Explicit custom mapping wins.
  }
  if (rules.size > 64) throw Error("默认和自定义脱敏规则合计不能超过 64 条，请合并后重新准备");
  for (const rule of rules.values()) {
    if (bytes(rule.literal) < 3 || bytes(rule.literal) > 256 || /[\x00-\x1f\x7f\u202a-\u202e\u2066-\u2069]/.test(rule.literal)) {
      throw Error("脱敏原文需为 3–256 字节且不含控制字符；目录过长时请先调整项目目录");
    }
    if (!/^[A-Za-z0-9_-]{3,64}$/.test(rule.replacement)) throw Error("替代标记需为 3–64 位英文字母、数字、下划线或连字符");
  }
  return longestFirst([...rules.values()]);
}

type RuleDraft = { rules: string; aliases: string; edited: boolean; prepared?: string };
const emptyDraft: RuleDraft = { rules: "", aliases: "", edited: false };
// Rules can contain sensitive source literals. Keep drafts in this app session,
// scoped to data directory + project + root; never write them to localStorage.
const drafts = new Map<string, RuleDraft>();
export const readReviewRuleDraft = (key: string) => drafts.get(key) ?? emptyDraft;
export const writeReviewRuleDraft = (key: string, draft: RuleDraft) => { drafts.set(key, draft); };
export const reviewRuleFingerprint = (data: Snapshot | null, projectId: string) => {
  const draft = readReviewRuleDraft(reviewRulesKey(data, projectId));
  return JSON.stringify([defaultReviewRedactions(data, projectId), draft.rules, draft.aliases]);
};
export const reviewRulesNeedPreparation = (data: Snapshot | null, projectId: string) => {
  const draft = readReviewRuleDraft(reviewRulesKey(data, projectId));
  return draft.edited && draft.prepared !== reviewRuleFingerprint(data, projectId);
};
