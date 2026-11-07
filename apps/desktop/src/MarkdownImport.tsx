import { HelpHeading } from "./HelpHeading";
import { useEffect, useId, useImperativeHandle, useMemo, useRef, useState, type Ref } from "react";
import { ErrorText } from "./ErrorText";
import { ImportReviewDialog } from "./ImportReviewDialog";
import {
  localMarkdownTool,
  refineLocalMarkdown,
  localRefinementText,
  localRefinementLabels,
  type LocalPoint,
} from "./LocalMarkdownRefinement";
export type MarkdownPlan = { title: string; tasks: string[] };

export async function readMarkdown(file: File, active: () => boolean = () => true,
  progress: (done: number, total: number) => void = () => {}): Promise<string> {
  if (!/\.md$/i.test(file.name))
    throw Error("仅支持 .md 文档，请选择 Markdown 文件。");
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const parts: string[] = [];
  const chunkBytes = 256 * 1024;
  for (let offset = 0; offset < file.size; offset += chunkBytes) {
    if (!active()) throw Error("文档或草稿已变化，已停止本地提炼。");
    const part = file.size <= chunkBytes ? file : file.slice(offset, offset + chunkBytes);
    const bytes = await part.arrayBuffer();
    if (!active()) throw Error("文档或草稿已变化，已停止本地提炼。");
    parts.push(decoder.decode(bytes, { stream: true }));
    progress(Math.min(offset + chunkBytes, file.size), file.size);
    await new Promise<void>(resolve => setTimeout(resolve, 0));
  }
  parts.push(decoder.decode());
  if (!active()) throw Error("文档或草稿已变化，已停止本地提炼。");
  const text = parts.join("").replace(/^\uFEFF/, "");
  if (!text.trim()) throw Error("文档为空，请选择包含正文的 Markdown 文件。");
  if (/[\u0000-\u0008\u000B\u000C\u000E-\u001F]/.test(text))
    throw Error("文档含有非文本内容，请保存为 UTF-8 Markdown 后重试。");
  return text.replace(/\r\n?/g, "\n");
}

type Review = {
  filename: string;
  source: string;
  scope: string;
  points: LocalPoint[];
  initialDraft: string;
  tasks: string[];
};
export function MarkdownImport({
  mode,
  contextKey,
  disabled,
  onApply,
  compact = false,
  fileImportRef,
}: {
  mode: "plan" | "requirement";
  contextKey: string;
  disabled: boolean;
  compact?: boolean;
  fileImportRef?: Ref<(file: File) => void>;
  onApply: (text: string, plan?: MarkdownPlan) => void;
}) {
  const [reading, setReading] = useState(false),
    [error, setError] = useState(""),
    [progress, setProgress] = useState("");
  const [review, setReview] = useState<Review>(),
    [draft, setDraft] = useState(""),
    [title, setTitle] = useState(""),
    [evidence, setEvidence] = useState(false);
  const [open, setOpen] = useState(false);
  const [workProgress, setWorkProgress] = useState({ done: 0, total: 1 });
  const [sourcePage, setSourcePage] = useState(0), [quotePage, setQuotePage] = useState(0);
  const sourcePageSize = 256 * 1024, quotePageSize = 128;
  const quotes = useMemo(() => evidence && review ? review.points.slice(quotePage * quotePageSize, (quotePage + 1) * quotePageSize)
    .map(point => `第 ${point.startLine}–${point.endLine} 行 · ${localRefinementLabels[point.kind]}\n${point.quote}`).join("\n\n") : "", [evidence, review, quotePage]);
  const fileInput = useRef<HTMLInputElement>(null), fieldId = useId();
  const scope = JSON.stringify([contextKey, mode]);
  const generation = useRef(0),
    latest = useRef({ scope, disabled, onApply });
  latest.current = { scope, disabled, onApply };
  useEffect(() => {
    generation.current++;
    setReview(undefined);
    setReading(false);
    setError("");
    setProgress("");
    setOpen(false);
    return () => {
      generation.current++;
    };
  }, [scope, disabled]);
  function cancel() {
    generation.current++;
    setReading(false);
    setReview(undefined);
    setOpen(false);
    setError("");
    setProgress(
      "已取消本地导入，原草稿保持不变。",
    );
  }
  async function select(file?: File) {
    const token = ++generation.current;
    setReview(undefined);
    setError("");
    setProgress("");
    setEvidence(false);
    setSourcePage(0); setQuotePage(0); setWorkProgress({ done: 0, total: 1 });
    if (!file || disabled) {
      setReading(false);
      return;
    }
    setOpen(true);
    const active = () =>
      generation.current === token &&
      latest.current.scope === scope &&
      !latest.current.disabled;
    setReading(true);
    try {
      setProgress("正在分段读取 Markdown 文档…");
      const source = await readMarkdown(file, active, (done, total) => {
        if (active()) {
          setWorkProgress({ done, total });
          setProgress(`读取文档：已完成 ${Math.ceil(done / (256 * 1024))}/${Math.ceil(total / (256 * 1024))} 段。`);
        }
      });
      if (!active()) return;
      const refined = await refineLocalMarkdown(
        source,
        file.name,
        active,
        (done, total) => {
          if (active()) {
            setWorkProgress({ done, total });
            setProgress(
              `分段提炼：已处理 ${Math.floor(done / Math.max(total, 1) * 100)}%。`,
            );
          }
        },
      );
      if (!active()) return;
      const result = await localRefinementText(refined, active, (done, total) => {
        if (active()) {
          setWorkProgress({ done, total });
          setProgress(`正在合并分段结果：${Math.floor(done / Math.max(total, 1) * 100)}%。`);
        }
      });
      if (!active()) return;
      const nextDraft = mode === "plan" ? result.tasks.join("\n\n") : result.text;
      setTitle(Array.from(result.title).slice(0, 150).join(""));
      setDraft(nextDraft);
      setReview({
        filename: file.name,
        source,
        scope,
        points: refined.points,
        initialDraft: nextDraft,
        tasks: result.tasks,
      });
      setProgress(
        `分段提炼完成，共整理 ${refined.lines} 行、${refined.points.length} 条内容。请对照原文核对归类、目标、任务与验收标准，确认后读入草稿。`,
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
            tasks: draft === review.initialDraft ? review.tasks : draft.split(/\n\s*\n/).filter((line) => line.trim()),
          }
        : undefined,
    );
    setReview(undefined);
    setOpen(false);
    setProgress("已确认读入草稿，保存、批准和启动仍需分别操作。");
  }
  useImperativeHandle(fileImportRef, () => file => { void select(file); });
  const input = <input ref={fileInput} type="file" accept=".md" hidden={compact}
    aria-label="导入本地 Markdown 文档（仅 .md）" disabled={disabled || reading}
    onChange={event => { const file = event.currentTarget.files?.[0]; event.currentTarget.value = ""; void select(file); }} />;
  const content = <>
      <p className="secondary">
        选择文档 → 本地自动提炼 → 审阅结果 →
        确认读入。不设文件大小、行数和正文长度上限；保留原文和行号供核对，实际处理受本机资源约束。
      </p>
      <p className="secondary" role="status">
        已绑定{localMarkdownTool.name}。按标题、列表和关键词整理，无法确定的段落保留在“背景与补充”；不推断缺失含义。此步骤离线运行，不调用模型，无需 Key 或主指挥配置。
      </p>
      {progress && <div className="markdown-import-progress"><p role="status">{progress}</p>
        {reading && <progress aria-label="文档处理进度" max={Math.max(workProgress.total, 1)} value={workProgress.done}/>}
      </div>}
      {error && (
        <p role="alert">
          <ErrorText error={error} />
        </p>
      )}
      {reading && (
        <button type="button" className="secondary-button" onClick={cancel}>
          停止本地提炼
        </button>
      )}
      {review && (
        <div className="panel" aria-label="本地文档提炼预览">
          <HelpHeading as="h4" topic="MarkdownImport">{review.filename} · 待确认</HelpHeading>
          {mode === "plan" && (
            <>
              <label htmlFor={`${fieldId}-title`}>提炼后的计划名称</label>
              <input
                id={`${fieldId}-title`}
                value={title}
                maxLength={200}
                onChange={(e) => setTitle(e.target.value)}
              />
            </>
          )}
          <label htmlFor={`${fieldId}-draft`}>提炼内容，可修改</label>
            <textarea
              id={`${fieldId}-draft`}
              rows={12}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
            />
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
                  value={review.source.slice(sourcePage * sourcePageSize, (sourcePage + 1) * sourcePageSize)}
                />
              </label>
              {review.source.length > sourcePageSize && <div className="markdown-evidence-pages" aria-label="原文分段浏览">
                <button type="button" className="secondary-button" disabled={sourcePage === 0} onClick={() => setSourcePage(sourcePage - 1)}>上一段原文</button>
                <span>第 {sourcePage + 1} / {Math.ceil(review.source.length / sourcePageSize)} 段</span>
                <button type="button" className="secondary-button" disabled={(sourcePage + 1) * sourcePageSize >= review.source.length} onClick={() => setSourcePage(sourcePage + 1)}>下一段原文</button>
              </div>}
              <label>
                提炼依据与原文行号
                <textarea
                  aria-label="提炼依据与原文行号"
                  readOnly
                  rows={8}
                  value={quotes}
                />
              </label>
              {review.points.length > quotePageSize && <div className="markdown-evidence-pages" aria-label="提炼依据分段浏览">
                <button type="button" className="secondary-button" disabled={quotePage === 0} onClick={() => setQuotePage(quotePage - 1)}>上一段依据</button>
                <span>第 {quotePage + 1} / {Math.ceil(review.points.length / quotePageSize)} 段</span>
                <button type="button" className="secondary-button" disabled={(quotePage + 1) * quotePageSize >= review.points.length} onClick={() => setQuotePage(quotePage + 1)}>下一段依据</button>
              </div>}
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
    </>;
  if (compact) return <div className="requirement-import-action markdown-import-compact">
    <button type="button" className="requirement-import-button" aria-label="导入 Markdown 文档" title="导入 Markdown 文档（本地提炼）"
      disabled={disabled || reading} onClick={() => fileInput.current?.click()}>
      <svg viewBox="0 0 24 24" width="21" height="21" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m20.8 11.6-8.4 8.4a6 6 0 0 1-8.5-8.5l9.2-9.2a4 4 0 0 1 5.7 5.7l-9.2 9.2a2 2 0 0 1-2.8-2.8l8.4-8.4"/></svg>
    </button>
    {input}<ImportReviewDialog open={open} title="Markdown 本地提炼" onClose={cancel}>{content}</ImportReviewDialog>
  </div>;
  return <section className="markdown-import" aria-label="导入 Markdown 文档"><label>导入本地 Markdown 文档（仅 .md）{input}</label>{content}</section>;
}
