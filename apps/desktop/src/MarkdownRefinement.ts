import { invoke } from "@tauri-apps/api/core";
import type {
  CommandResult,
  DocumentRefinementRequest,
  RefinedDocument,
  WorkspaceResult,
  SavedProjectSetup,
} from "./contracts";

export type RefinementApi = {
  digest: (text: string) => Promise<string>;
  id: () => string;
  part: (
    input: DocumentRefinementRequest,
    active: () => boolean,
  ) => Promise<RefinedDocument>;
};
export function documentParts(text: string): string[] {
  const parts: string[] = [];
  let part = "",
    bytes = 0;
  for (const char of text) {
    const cp = char.codePointAt(0)!;
    const size = cp <= 0x7f ? 1 : cp <= 0x7ff ? 2 : cp <= 0xffff ? 3 : 4;
    if (bytes + size > 6000) {
      if (part.trim()) parts.push(part);
      part = "";
      bytes = 0;
    }
    part += char;
    bytes += size;
  }
  if (part.trim()) parts.push(part);
  return parts;
}
const pause = () => new Promise((resolve) => setTimeout(resolve, 1000));
export function createNativeRefinement(
  call: typeof invoke = invoke,
): RefinementApi {
  return {
    id: () => crypto.randomUUID(),
    digest: async (text) =>
      Array.from(
        new Uint8Array(
          await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)),
        ),
        (byte) => byte.toString(16).padStart(2, "0"),
      ).join(""),
    part: async (input, active) => {
      if (!active()) throw Error("原提炼请求已失效。");
      const sent = await call<CommandResult>("core_submit", {
        command: { type: "requestDocumentRefinement", input },
      });
      if (sent.type !== "receipt")
        throw Error(
          "提炼请求未返回确认，请在指挥记录中核对原请求，不会自动重发。",
        );
      const deadline = Date.now() + 120_000;
      while (active() && Date.now() < deadline) {
        let offset = 0,
          revision: string | null = null;
        while (active()) {
          const list: WorkspaceResult = await call<WorkspaceResult>(
            "workspace_read",
            {
              query: {
                type: "managementDecisions",
                projectId: input.projectId,
                offset,
                revision,
              },
            },
          );
          if (!active()) break;
          if (
            list.type !== "managementDecisions" ||
            list.value.projectId !== input.projectId ||
            list.value.offset !== offset ||
            (revision !== null && list.value.revision !== revision)
          )
            throw Error("无法读取原提炼记录。");
          revision = list.value.revision;
          const status = list.value.entries.find(
            (item) => item.operationId === input.operationId,
          );
          if (status) {
            if (
              ["unknown", "discarded", "abandoned", "startupClosed"].includes(
                status.state,
              )
            )
              throw Error(
                status.message ||
                  "模型请求未成功完成，请核对原指挥记录；不会自动重试。",
              );
            if (status.state === "applied" || status.message) {
              const detail = await call<WorkspaceResult>("workspace_read", {
                query: {
                  type: "managementDecision",
                  projectId: input.projectId,
                  operationId: input.operationId,
                  revision,
                },
              });
              if (!active()) break;
              if (
                detail.type !== "managementDecision" ||
                detail.value.projectId !== input.projectId ||
                detail.value.revision !== revision ||
                detail.value.record.operationId !== input.operationId ||
                detail.value.record.purpose !== "refine"
              )
                throw Error("提炼结果身份不符。");
              const record = detail.value.record;
              if (record.state === "applied" && record.refinement)
                return record.refinement;
              throw Error(
                record.message ||
                  record.waitReason ||
                  "模型未返回完整提炼结果，请核对原记录。",
              );
            }
            break;
          }
          if (list.value.nextOffset === null) break;
          if (list.value.nextOffset <= offset)
            throw Error("提炼记录分页无效。");
          offset = list.value.nextOffset;
        }
        await pause();
      }
      throw Error(
        active()
          ? "提炼等待超时，原请求可能仍在运行，请核对指挥记录；不会重发。"
          : "已停止后续分段，原请求和计费结果仍保留。",
      );
    },
  };
}
export const nativeRefinement = createNativeRefinement();

export async function refineDocument(
  source: string,
  chief: SavedProjectSetup,
  api: RefinementApi,
  active: () => boolean,
  progress: (done: number, total: number) => void,
): Promise<RefinedDocument[]> {
  const parts = documentParts(source),
    digest = await api.digest(source),
    id = api.id();
  if (!parts.length) throw Error("文档没有可提炼的正文。");
  const result: RefinedDocument[] = [];
  for (let part = 0; part < parts.length; part++) {
    if (!active()) throw Error("文档或主指挥配置已变化，已停止后续提炼。");
    progress(part, parts.length);
    const value = await api.part(
      {
        projectId: chief.setup.projectId,
        setupDigest: chief.digest,
        operationId: `md-${id}-${part}`,
        documentDigest: digest,
        part,
        total: parts.length,
        text: parts[part],
      },
      active,
    );
    if (!active()) throw Error("原提炼结果已失效。");
    if (
      !value.title?.trim() ||
      !value.points?.length ||
      value.points.some(
        (point) =>
          !point.text?.trim() ||
          !point.quote?.trim() ||
          !parts[part].includes(point.quote),
      )
    )
      throw Error("提炼结果缺少正文或原文引用，未读入草稿。");
    result.push(value);
  }
  progress(parts.length, parts.length);
  return result;
}

export const refinementLabels = {
  objective: "目标",
  task: "任务",
  acceptance: "验收标准",
  constraint: "约束",
  question: "待澄清问题",
  context: "背景与补充",
} as const;
export function refinementText(documents: RefinedDocument[]): {
  title: string;
  text: string;
  tasks: string[];
} {
  const title = Array.from(documents[0].title).slice(0, 150).join("");
  const sections: string[] = [],
    tasks: string[] = [];
  for (const [kind, label] of Object.entries(refinementLabels)) {
    const points = documents.flatMap((document) =>
      document.points.filter((point) => point.kind === kind),
    );
    const unique = [...new Set(points.map((point) => point.text))];
    if (unique.length) {
      sections.push(
        `${label}\n${unique.map((text) => `- ${text}`).join("\n")}`,
      );
      tasks.push(...unique.map((text) => `${label}：${text}`));
    }
  }
  return { title, text: [title, ...sections].join("\n\n"), tasks };
}
