import type { ProjectWorkerAdapter } from "./contracts";

export type WorkerMethod = "api" | "cli" | "desktop";

export const workerMethodLabels: Record<WorkerMethod, string> = {
  api: "API 连接",
  cli: "CLI 接入",
  desktop: "桌面端接入",
};

export function workerMethod(adapter: ProjectWorkerAdapter = "codex"): WorkerMethod {
  if (adapter === "codex" || adapter === "omp") return "cli";
  if (adapter === "qoder" || adapter === "workbuddy") return "desktop";
  return "api";
}

export const defaultWorkerAdapter: Record<WorkerMethod, ProjectWorkerAdapter> = {
  api: "deepseek",
  cli: "codex",
  desktop: "qoder",
};

// Validation and explicit navigation must reveal the field before focusing it.
// Native details groups enforce the managers' single-open-panel rule.
export function revealConnectionField(field: HTMLElement) {
  const ancestors: HTMLDetailsElement[] = [];
  for (let parent = field.parentElement; parent; parent = parent.parentElement) {
    if (parent instanceof HTMLDetailsElement) ancestors.push(parent);
  }
  for (const details of ancestors.reverse()) details.open = true;
  field.focus();
}
