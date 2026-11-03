import { useEffect, useRef, useState } from "react";

export type MarkdownPlan = { title: string; tasks: string[] };
const bytes = (text: string) => new TextEncoder().encode(text).length;

export async function readMarkdown(file: File): Promise<string> {
  if (!/\.md$/i.test(file.name)) throw Error("仅支持 .md 文档，请选择 Markdown 文件。");
  if (file.size > 64 * 1024) throw Error("Markdown 文档不得超过 64 KiB，请拆分后导入。");
  const text = new TextDecoder("utf-8", { fatal: true }).decode(await file.arrayBuffer()).replace(/^\uFEFF/, "");
  if (!text.trim()) throw Error("Markdown 文档为空。");
  if (/[\u0000-\u0008\u000B\u000C\u000E-\u001F]/.test(text)) throw Error("文档含有非文本内容，请保存为 UTF-8 Markdown 后重试。");
  return text.replace(/\r\n?/g, "\n");
}

export function markdownPlan(text: string, filename: string): MarkdownPlan {
  // Do not execute/render Markdown or follow links. Keep the complete body;
  // only an initial H1 becomes the title. Nonempty lines retain existing plan semantics.
  const lines = text.split("\n");
  const first = lines.findIndex(line => line.trim());
  const heading = first >= 0 ? /^#\s+(.+?)\s*$/.exec(lines[first]) : null;
  const title = heading ? heading[1] : filename.replace(/\.md$/i, "");
  const tasks = lines.filter((line, i) => i !== (heading ? first : -1) && line.trim());
  if (!title.trim() || title.length > 200 || bytes(title) > 600) throw Error("计划名称过长，请缩短文档首行标题或文件名。");
  if (!tasks.length || tasks.length > 30 || tasks.some(task => bytes(task) > 2000)) {
    throw Error("文档正文须有 1–30 个非空行，每行最多 2000 字节；不会截断内容，请整理后导入。");
  }
  return { title, tasks };
}

type Review = { filename: string; text: string; plan?: MarkdownPlan };
export function MarkdownImport({ mode, contextKey, disabled, onApply }: {
  mode: "plan" | "requirement";
  contextKey: string;
  disabled: boolean;
  onApply: (text: string, plan?: MarkdownPlan) => void;
}) {
  const [review, setReview] = useState<Review>();
  const [reading, setReading] = useState(false), [error, setError] = useState("");
  const generation = useRef(0), latest = useRef({ contextKey, disabled });
  latest.current = { contextKey, disabled };
  useEffect(() => {
    generation.current++; setReview(undefined); setError(""); setReading(false);
    return () => { generation.current++; };
  }, [contextKey, disabled]);
  async function select(file?: File) {
    const token = ++generation.current, context = contextKey;
    setReview(undefined); setError("");
    if (!file || disabled) { setReading(false); return; }
    const active = () => generation.current === token && latest.current.contextKey === context && !latest.current.disabled;
    setReading(true);
    try {
      const text = await readMarkdown(file);
      if (mode === "requirement" && bytes(text) > 8000) throw Error("项目需求最多 8000 字节；文档不会被截断，请精简后导入。");
      const plan = mode === "plan" ? markdownPlan(text, file.name) : undefined;
      if (active()) setReview({ filename: file.name, text, plan });
    } catch (e) {
      if (active()) setError(e instanceof TypeError ? "无法按 UTF-8 读取文档，请另存为 UTF-8 .md 文件。" : String(e).replace(/^Error: /, ""));
    } finally { if (active()) setReading(false); }
  }
  return <section className="markdown-import" aria-label="导入 Markdown 文档">
    <label>导入本地 Markdown 文档（仅 .md）<input type="file" accept=".md" disabled={disabled || reading} onChange={e => { const file = e.currentTarget.files?.[0]; e.currentTarget.value = ""; void select(file); }}/></label>
    <p className="secondary">{mode === "plan" ? "首个非空行若为一级标题，将用作计划名称；正文每个非空行对应一项任务。" : "文档原文将填入项目需求，可继续编辑。"}只在本机读取文本，不执行文档内容或加载外部链接；确认导入不会自动保存、调用模型或批准任务。</p>
    {reading && <p role="status">正在读取文档…</p>}
    {error && <p role="alert">{error}</p>}
    {review && <div className="panel" aria-label="Markdown 导入预览">
      <h4>{review.filename}</h4>
      {review.plan && <p>计划名称：{review.plan.title} · {review.plan.tasks.length} 项任务</p>}
      <label>文档原文预览<textarea readOnly rows={8} value={review.text}/></label>
      <p>确认后将替换当前{mode === "plan" ? "计划名称与任务" : "需求"}草稿。请核对内容，保存和批准仍需分别操作。</p>
      <div className="integration-actions"><button type="button" className="secondary-button" disabled={disabled || reading} onClick={() => setReview(undefined)}>取消导入</button>
        <button type="button" className="primary" disabled={disabled || reading} onClick={() => { if (!latest.current.disabled && latest.current.contextKey === contextKey) { onApply(review.text, review.plan); setReview(undefined); } }}>确认导入 Markdown</button></div>
    </div>}
  </section>;
}
