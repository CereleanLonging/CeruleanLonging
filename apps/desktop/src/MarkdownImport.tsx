import { HelpHeading } from "./HelpHeading";
import { useEffect, useRef, useState } from "react";
import { ErrorText } from "./ErrorText";
import type { SavedProjectSetup } from "./contracts";
import {
  nativeRefinement,
  refineDocument,
  refinementText,
  type RefinementApi,
} from "./MarkdownRefinement";
export type MarkdownPlan = { title: string; tasks: string[] };

export async function readMarkdown(file: File): Promise<string> {
  if (!/\.md$/i.test(file.name))
    throw Error("仅支持 .md 文档，请选择 Markdown 文件。");
  const text = new TextDecoder("utf-8", { fatal: true })
    .decode(await file.arrayBuffer())
    .replace(/^\uFEFF/, "");
  if (!text.trim()) throw Error("文档为空，请选择包含正文的 Markdown 文件。");
  if (/[\u0000-\u0008\u000B\u000C\u000E-\u001F]/.test(text))
    throw Error("文档含有非文本内容，请保存为 UTF-8 Markdown 后重试。");
  return text.replace(/\r\n?/g, "\n");
}

type Review = {
  filename: string;
  source: string;
  scope: string;
  quotes: string[];
};
export function MarkdownImport({
  mode,
  contextKey,
  disabled,
  onApply,
  chief,
  api = nativeRefinement,
}: {
  mode: "plan" | "requirement";
  contextKey: string;
  disabled: boolean;
  chief?: SavedProjectSetup;
  api?: RefinementApi;
  onApply: (text: string, plan?: MarkdownPlan) => void;
}) {
  const [reading, setReading] = useState(false),
    [error, setError] = useState(""),
    [progress, setProgress] = useState("");
  const [review, setReview] = useState<Review>(),
    [draft, setDraft] = useState(""),
    [title, setTitle] = useState(""),
    [evidence, setEvidence] = useState(false);
  const scope = JSON.stringify([
    contextKey,
    chief?.setup.projectId,
    chief?.digest,
  ]);
  const generation = useRef(0),
    latest = useRef({ scope, disabled, onApply });
  latest.current = { scope, disabled, onApply };
  useEffect(() => {
    generation.current++;
    setReview(undefined);
    setReading(false);
    setError("");
    setProgress("");
    return () => {
      generation.current++;
    };
  }, [scope, disabled]);
  const external =
    chief?.setup.managementSources?.chief.kind === "codexSession";
  function cancel() {
    generation.current++;
    setReading(false);
    setReview(undefined);
    setError("");
    setProgress(
      "已取消读入，并停止后续分段。已发出的模型请求仍保留在指挥调用记录中。",
    );
  }
  async function select(file?: File) {
    const token = ++generation.current;
    setReview(undefined);
    setError("");
    setProgress("");
    setEvidence(false);
    if (!file || disabled || !chief || external) {
      setReading(false);
      return;
    }
    const active = () =>
      generation.current === token &&
      latest.current.scope === scope &&
      !latest.current.disabled;
    setReading(true);
    try {
      const source = await readMarkdown(file);
      if (!active()) return;
      const refined = await refineDocument(
        source,
        chief,
        api,
        active,
        (done, total) => {
          if (active())
            setProgress(
              `主指挥自动提炼：${done}/${total} 段。每段发送一次，失败不自动重试。`,
            );
        },
      );
      if (!active()) return;
      const result = refinementText(refined);
      setTitle(result.title);
      setDraft(mode === "plan" ? result.tasks.join("\n\n") : result.text);
      setReview({
        filename: file.name,
        source,
        scope,
        quotes: refined.flatMap((doc) =>
          doc.points.map((point) => point.quote),
        ),
      });
      setProgress(
        "提炼完成。请对照原文核对目标、任务与验收标准，确认后才会替换草稿。",
      );
    } catch (e) {
      if (active())
        setError(
          e instanceof TypeError
            ? "无法读取文档或提炼结果，请核对 UTF-8 编码及原调用记录。"
            : String(e).replace(/^Error: /, ""),
        );
    } finally {
      if (active()) setReading(false);
    }
  }
  function apply() {
    if (
      !review ||
      reading ||
      latest.current.disabled ||
      review.scope !== latest.current.scope
    )
      return;
    if (!draft.trim()) {
      setError("提炼内容不能为空，请核对后确认。");
      return;
    }
    latest.current.onApply(
      draft,
      mode === "plan"
        ? {
            title: title.trim() || review.filename.replace(/\.md$/i, ""),
            tasks: draft.split(/\n\s*\n/).filter((line) => line.trim()),
          }
        : undefined,
    );
    setReview(undefined);
    setProgress("已确认读入草稿，保存、批准和启动仍需分别操作。");
  }
  return (
    <section className="markdown-import" aria-label="导入 Markdown 文档">
      <label>
        导入本地 Markdown 文档（仅 .md）
        <input
          type="file"
          accept=".md"
          disabled={disabled || reading || !chief || external}
          onChange={(e) => {
            const file = e.currentTarget.files?.[0];
            e.currentTarget.value = "";
            void select(file);
          }}
        />
      </label>
      <p className="secondary">
        选择文档 → 主指挥自动提炼 → 审阅结果 →
        确认读入。不设文件大小、行数和正文长度上限；长文档会分段提炼，保留原文供核对。实际处理受本机资源、模型能力及项目调用额度约束。
      </p>
      {chief ? (
        <p className="secondary">
          使用已保存的主指挥：{chief.setup.chiefModel}
          。文档内容会发送至该模型，可能计费，并计入项目指挥请求额度。
        </p>
      ) : (
        <p role="status">请先保存并选择一个已配置主指挥的项目。</p>
      )}
      {external && (
        <p role="status">
          此主指挥绑定外部 Codex 聊天，不能后台自动提炼。请在项目配置中选择 API
          主指挥；不会改用其他模型。
        </p>
      )}
      {progress && <p role="status">{progress}</p>}
      {error && (
        <p role="alert">
          <ErrorText error={error} />
        </p>
      )}
      {reading && (
        <button type="button" className="secondary-button" onClick={cancel}>
          停止后续提炼
        </button>
      )}
      {review && (
        <div className="panel" aria-label="主指挥文档提炼预览">
          <HelpHeading as="h4" topic="MarkdownImport">{review.filename} · 待确认</HelpHeading>
          {mode === "plan" && (
            <label>
              提炼后的计划名称
              <input
                value={title}
                maxLength={200}
                onChange={(e) => setTitle(e.target.value)}
              />
            </label>
          )}
          <label>
            提炼内容，可修改
            <textarea
              rows={12}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
            />
          </label>
          <button
            type="button"
            className="secondary-button"
            onClick={() => setEvidence(!evidence)}
          >
            {evidence ? "收起原文与引用" : "查看原文与引用"}
          </button>
          {evidence && (
            <>
              <label>
                原文
                <textarea
                  aria-label="原文"
                  readOnly
                  rows={12}
                  value={review.source}
                />
              </label>
              <label>
                模型引用原文片段
                <textarea
                  aria-label="模型引用原文片段"
                  readOnly
                  rows={8}
                  value={review.quotes.join("\n\n")}
                />
              </label>
            </>
          )}
          <div className="integration-actions">
            <button type="button" className="secondary-button" onClick={cancel}>
              取消导入
            </button>
            <button
              type="button"
              className="primary"
              disabled={disabled || reading}
              onClick={apply}
            >
              确认读入草稿
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
