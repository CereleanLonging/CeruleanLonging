import { useEffect, useRef, useState } from "react";
import type { CodexStartupSummary, WorkspaceCodexStartupDetail, WorkspaceCodexStartupPage } from "./contracts";
import type { WorkspaceReader } from "./WorkspaceBrowser";

const states: Record<string, string> = { prepared: "启动准备已记录", advanced: "启动已进入后续流程", closed: "启动记录已关闭", legacy: "旧记录缺少启动证据" };
type Props = { projectId: string; read: WorkspaceReader; busy: boolean };
function validSummary(value: CodexStartupSummary, projectId: string) {
  return value.projectId === projectId && !!value.taskId && !!value.attemptId && Object.hasOwn(states, value.state) && (value.managementOperation === null || typeof value.managementOperation === "string") && (value.intentHash === null || typeof value.intentHash === "string");
}
function sameSummary(a: CodexStartupSummary, b: CodexStartupSummary) {
  return a.projectId === b.projectId && a.taskId === b.taskId && a.attemptId === b.attemptId && a.state === b.state && a.intentHash === b.intentHash && a.managementOperation === b.managementOperation;
}
function validPage(value: WorkspaceCodexStartupPage, offset: number) {
  const end = offset + value.entries.length;
  return value.offset === offset && Number.isSafeInteger(offset) && offset >= 0 && Number.isSafeInteger(value.total) && value.total >= offset && value.entries.length <= 20 && end <= value.total && (value.total === 0 || value.entries.length > 0) && (end === value.total ? value.nextOffset === null : value.nextOffset === end);
}
export function WorkspaceCodexStartups(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(false), lock = useRef(false), generation = useRef(0), detailGeneration = useRef(0), shown = useRef<WorkspaceCodexStartupPage | null>(null);
  const [page, setPage] = useState<WorkspaceCodexStartupPage | null>(null), [back, setBack] = useState<number[]>([]);
  const [detail, setDetail] = useState<WorkspaceCodexStartupDetail | null>(null);
  const [loading, setLoading] = useState(false), [stale, setStale] = useState(false), [error, setError] = useState("");
  const blocked = loading || props.busy;
  async function run(action: (current: () => boolean) => Promise<void>) {
    if (!alive.current || lock.current || latest.current.busy) return;
    lock.current = true; const token = ++generation.current, projectId = latest.current.projectId;
    const current = () => alive.current && generation.current === token && latest.current.projectId === projectId;
    setLoading(true);
    try { await action(current); if (current()) setError(""); }
    catch (e) { if (current()) { setError(String(e)); setStale(true); } }
    finally { if (current()) { lock.current = false; setLoading(false); } }
  }
  async function readPage(offset: number, revision: string | null, history: number[], current: () => boolean) {
    const projectId = latest.current.projectId;
    const reply = await latest.current.read({ type: "codexStartups", projectId, offset, revision });
    if (!current()) return;
    if (new TextEncoder().encode(JSON.stringify(reply)).length > 64 * 1024) throw Error("原启动页超过有界读取大小，请刷新。");
    if (reply.type !== "codexStartups" || reply.value.projectId !== projectId || !reply.value.revision || (revision !== null && (reply.value.revision !== revision || reply.value.total !== shown.current?.total)) || !validPage(reply.value, offset) || reply.value.entries.some(s => !validSummary(s, projectId)) || new Set(reply.value.entries.map(s => s.attemptId)).size !== reply.value.entries.length) throw Error("原启动页身份、游标或版本不符，请刷新。");
    const previous = shown.current;
    if (previous?.revision === reply.value.revision && (previous.total !== reply.value.total || (previous.offset === offset && (previous.nextOffset !== reply.value.nextOffset || JSON.stringify(previous.entries) !== JSON.stringify(reply.value.entries))))) throw Error("同一启动版本的原内容不一致，请刷新。");
    if (shown.current?.revision !== reply.value.revision || shown.current?.offset !== offset) setDetail(null);
    shown.current = reply.value; setPage(reply.value); setBack(history); setStale(false);
  }
  useEffect(() => {
    alive.current = true; lock.current = false; shown.current = null; setPage(null); setDetail(null); setBack([]); setLoading(false); setStale(false); setError("");
    void run(current => readPage(0, null, [], current));
    return () => { alive.current = false; generation.current++; lock.current = false; };
  }, [props.projectId]);
  useEffect(() => {
    if (!page || stale || blocked || !page.entries.some(s => s.state === "prepared" || s.state === "advanced")) return;
    // Read only a bounded page. Never poll snapshot or execution commands.
    const timer = window.setTimeout(() => void run(current => readPage(page.offset, null, back, current)), 2000);
    return () => window.clearTimeout(timer);
  }, [page, stale, blocked, back]);
  useEffect(() => {
    if (!page && !loading && !stale && !props.busy) void run(current => readPage(0, null, [], current));
  }, [page, loading, stale, props.busy]);
  function inspect(summary: CodexStartupSummary) {
    if (!page || lock.current || latest.current.busy || !alive.current) return;
    const projectId = props.projectId, revision = page.revision, detailToken = ++detailGeneration.current;
    void run(async current => {
      const reply = await latest.current.read({ type: "codexStartup", projectId, attemptId: summary.attemptId, revision });
      if (!current() || detailGeneration.current !== detailToken) return;
      if (new TextEncoder().encode(JSON.stringify(reply)).length > 512 * 1024) throw Error("原启动详情超过有界读取大小，请刷新。");
      if (reply.type !== "codexStartup" || reply.value.projectId !== projectId || reply.value.revision !== revision || !sameSummary(reply.value.detail.summary, summary) || !Number.isSafeInteger(reply.value.detail.workflowSequence) || reply.value.detail.workflowSequence < 0) throw Error("原启动详情身份、摘要或版本不符，请刷新。");
      setDetail(reply.value);
    });
  }
  return <section className="panel" style={{ overflowWrap: "anywhere" }} aria-label="项目编程启动记录" aria-busy={loading}>
    <div className="section-heading"><h3>项目 {props.projectId} 的编程启动记录</h3><button disabled={blocked} onClick={() => void run(current => readPage(0, null, [], current))}>刷新启动首页</button></div>
    <p>这里只读取原 Codex 启动意图与证据，尚无作业的启动也会列出。不启动、重试、取消、关闭或放弃任何尝试。</p>
    <p>启动准备、进入后续流程或记录已关闭都不等于任务完成；未知结果与资源占用须回原项目核对，不能据此重发。</p>
    {loading && <p role="status">正在读取原启动记录…</p>}
    {error && <p role="alert">{error} 上次内容仅供核对，请刷新后继续。</p>}
    {!page ? <p>启动记录尚未取得，不能据此判断没有启动意图。</p> : <>
      <p>原项目共 {page.total} 条启动记录</p>
      {page.total === 0 ? <p>原项目尚无 Codex 启动记录；可刷新核对最新记录。</p> : <ol start={page.offset + 1}>{page.entries.map(s => <li key={s.attemptId}><h4>{s.attemptId} · {states[s.state]}</h4><p>任务 {s.taskId} · 指挥请求 {s.managementOperation ?? "未关联指挥请求"}</p><p>启动意图摘要 <code>{s.intentHash ?? "旧记录未记录"}</code></p><button disabled={blocked || stale} onClick={() => inspect(s)}>查看启动 {s.attemptId} 的原证据</button></li>)}</ol>}
      <nav aria-label="启动记录分页"><button disabled={blocked || stale || !back.length} onClick={() => void run(current => readPage(back.at(-1)!, page.revision, back.slice(0, -1), current))}>上一页</button><span>当前显示 {page.entries.length} / {page.total}</span><button disabled={blocked || stale || page.nextOffset === null} onClick={() => void run(current => readPage(page.nextOffset!, page.revision, [...back, page.offset], current))}>下一页</button></nav>
    </>}
    {detail && <section aria-label="原启动证据"><div className="section-heading"><h4>尝试 {detail.detail.summary.attemptId} 的原启动证据</h4><button onClick={() => { detailGeneration.current++; setDetail(null); }}>关闭启动详情</button></div><p>{states[detail.detail.summary.state]} · 工作目录 {detail.detail.workspace}</p><p>模型 {detail.detail.workerModel} · 启动时间 {detail.detail.startedAt} · 协议 {detail.detail.protocol ?? "未记录"}</p><p>方案摘要 <code>{detail.detail.revisionDigest}</code> · 工作流序号 {detail.detail.workflowSequence}</p><p>证据摘要 <code>{detail.detail.evidenceDigest ?? "尚无可确认摘要"}</code></p>{detail.detail.blockedReason && <p>原阻断说明：{detail.detail.blockedReason}</p>}{detail.detail.reason && <p>原关闭说明：{detail.detail.reason}</p>}<details><summary>原启动完整字段</summary><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{JSON.stringify(detail, null, 2)}</pre></details></section>}
  </section>;
}
