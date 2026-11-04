import { HelpHeading } from "./HelpHeading";
import { useEffect, useRef, useState } from "react";
import type { WorkspaceProjectRunDetail, WorkspaceProjectRunPage, WorkspaceProjectRunSummary } from "./contracts";
import type { WorkspaceReader } from "./WorkspaceBrowser";

type Props = { projectId: string; read: WorkspaceReader; busy: boolean };

function validPage(value: WorkspaceProjectRunPage, projectId: string, offset: number, revision: string | null) {
  const end = offset + value.entries.length;
  return value.projectId === projectId && !!value.revision && (revision === null || value.revision === revision)
    && value.offset === offset && Number.isSafeInteger(offset) && offset >= 0
    && Number.isSafeInteger(value.total) && value.total >= 0 && value.total >= offset
    && Number.isSafeInteger(value.activeCount) && value.activeCount >= 0 && value.activeCount <= value.total
    && value.entries.length <= 20 && end <= value.total && (value.total === 0 ? value.entries.length === 0 : value.entries.length > 0)
    && (end === value.total ? value.nextOffset === null : value.nextOffset === end)
    && new Set(value.entries.map(entry => entry.id)).size === value.entries.length
    && value.entries.every(entry => entry.projectId === projectId && !!entry.id && !!entry.revisionDigest && !!entry.setupDigest && typeof entry.hasMessage === "boolean" && Number.isSafeInteger(entry.acknowledgedDecisions) && entry.acknowledgedDecisions >= 0 && ["active", "paused", "attention", "completed", "superseded"].includes(entry.state));
}

export function WorkspaceProjectRuns(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(true), lock = useRef(false), generation = useRef(0), detailGeneration = useRef(0), shown = useRef<WorkspaceProjectRunPage | null>(null);
  const [page, setPage] = useState<WorkspaceProjectRunPage | null>(null), [back, setBack] = useState<number[]>([]);
  const [detail, setDetail] = useState<WorkspaceProjectRunDetail | null>(null);
  const [loading, setLoading] = useState(false), [stale, setStale] = useState(false), [error, setError] = useState("");
  const blocked = loading || props.busy;

  async function run(action: (current: () => boolean) => Promise<void>) {
    if (lock.current || latest.current.busy) return;
    lock.current = true;
    const token = ++generation.current, projectId = latest.current.projectId;
    const current = () => alive.current && generation.current === token && latest.current.projectId === projectId;
    setLoading(true); setError("");
    try { await action(current); }
    catch (e) { if (current()) { setError(String(e)); setStale(true); } }
    finally { if (current()) { lock.current = false; setLoading(false); } }
  }

  async function readPage(offset: number, revision: string | null, history: number[], current: () => boolean) {
    const projectId = latest.current.projectId;
    const reply = await latest.current.read({ type: "projectRuns", projectId, offset, revision });
    if (!current()) return;
    if (new TextEncoder().encode(JSON.stringify(reply)).byteLength > 64 * 1024) throw Error("原项目运行页超过有界读取大小，请刷新。");
    if (reply.type !== "projectRuns" || !validPage(reply.value, projectId, offset, revision)) throw Error("原项目运行页身份、游标或版本不符，请刷新。");
    const previous = shown.current;
    if (previous?.revision === reply.value.revision && previous.total !== reply.value.total) throw Error("同一项目运行版本的总数不一致，请刷新。");
    if (previous?.revision === reply.value.revision && previous.activeCount !== reply.value.activeCount) throw Error("同一项目运行版本的活动计数不一致，请刷新。");
    if (previous?.revision === reply.value.revision && previous.offset === offset && (previous.nextOffset !== reply.value.nextOffset || JSON.stringify(previous.entries) !== JSON.stringify(reply.value.entries))) throw Error("同一项目运行版本的原摘要不一致，请刷新。");
    if (previous?.revision !== reply.value.revision || previous?.offset !== offset) { detailGeneration.current++; setDetail(null); }
    shown.current = reply.value;
    setPage(reply.value); setBack(history); setStale(false);
  }

  useEffect(() => {
    alive.current = true; lock.current = false; generation.current++; detailGeneration.current++; shown.current = null;
    setPage(null); setBack([]); setDetail(null); setStale(false); setError("");
    void run(current => readPage(0, null, [], current));
    return () => { alive.current = false; generation.current++; detailGeneration.current++; lock.current = false; };
  }, [props.projectId]);

  useEffect(() => {
    if (!props.busy && !page && !loading && !stale) void run(current => readPage(0, null, [], current));
  }, [props.busy, props.projectId, page, loading, stale]);

  useEffect(() => {
    // Keep the user's historical page pinned. Live refresh resumes on page 0.
    if (!page || page.offset !== 0 || page.activeCount <= 0 || stale || loading || props.busy) return;
    const timer = window.setTimeout(() => { if (!lock.current && !latest.current.busy) void run(current => readPage(0, null, [], current)); }, 1000);
    return () => window.clearTimeout(timer);
  }, [page, stale, loading, props.busy]);

  function readDetail(summary: WorkspaceProjectRunSummary) {
    if (!page || stale) return;
    const projectId = props.projectId, revision = page.revision, token = ++detailGeneration.current;
    void run(async current => {
      const reply = await latest.current.read({ type: "projectRun", projectId, runId: summary.id, revision });
      if (!current() || token !== detailGeneration.current) return;
      if (new TextEncoder().encode(JSON.stringify(reply)).byteLength > 512 * 1024) throw Error("原项目运行详情超过有界读取大小，请刷新。");
      if (reply.type !== "projectRun" || reply.value.projectId !== projectId || reply.value.revision !== revision || !reply.value.record || reply.value.record.id !== summary.id || reply.value.record.projectId !== projectId || reply.value.record.revisionDigest !== summary.revisionDigest || reply.value.record.setupDigest !== summary.setupDigest || reply.value.record.state !== summary.state || (reply.value.record.message.length > 0) !== summary.hasMessage || reply.value.record.acknowledgedDecisions !== summary.acknowledgedDecisions) throw Error("原项目运行详情身份或版本不符，请刷新。");
      setDetail(reply.value);
    });
  }

  return <section className="panel" aria-label="项目运行记录" aria-busy={loading}>
    <div className="section-heading"><HelpHeading as="h3" topic="WorkspaceProjectRuns">项目 {props.projectId} 的运行记录</HelpHeading><button disabled={blocked} onClick={() => void run(current => readPage(0, null, [], current))}>刷新运行首页</button></div>
    <p>这里只读原始项目运行记录；不会启动、暂停、取消或关闭运行。</p>
    {page && page.offset > 0 && page.activeCount > 0 && <p>正在查看历史页，自动刷新暂缓；返回首页后恢复活动状态刷新。</p>}
    {loading && <p role="status">正在读取原项目运行记录…</p>}
    {error && <p role="alert">{error} 上次内容仅供核对，请刷新后继续。</p>}
    {!page ? <p>项目运行记录尚未取得；这不表示项目没有运行。</p> : <>
      <p>原项目共 {page.total} 条运行记录；当前活动运行 {page.activeCount} 条；版本 <code>{page.revision}</code></p>
      {page.total === 0 ? <p>原项目尚无运行记录。</p> : page.entries.map(entry => <article className="panel" key={entry.id}>
        <HelpHeading as="h4" topic="WorkspaceProjectRuns">{entry.id}</HelpHeading><p>状态：{entry.state} · 修订摘要 <code>{entry.revisionDigest}</code> · 设置摘要 <code>{entry.setupDigest}</code></p>
        <p>{entry.hasMessage ? "包含原说明" : "没有原说明"} · 已确认决定 {entry.acknowledgedDecisions}</p>
        <button disabled={blocked || stale} onClick={() => readDetail(entry)}>查看完整原运行记录</button>
      </article>)}
      <nav aria-label="项目运行分页"><button disabled={blocked || stale || !back.length} onClick={() => void run(current => readPage(back.at(-1)!, page.revision, back.slice(0, -1), current))}>上一页</button><span>当前显示 {page.entries.length} / {page.total}</span><button disabled={blocked || stale || page.nextOffset === null} onClick={() => void run(current => readPage(page.nextOffset!, page.revision, [...back, page.offset], current))}>下一页</button></nav>
    </>}
    {detail && <section className="panel" aria-label="完整原项目运行记录"><HelpHeading as="h4" topic="WorkspaceProjectRuns">运行 {detail.record.id} 的完整原记录</HelpHeading><p>项目 {detail.record.projectId} · 状态 {detail.record.state} · 修订摘要 <code>{detail.record.revisionDigest}</code> · 设置摘要 <code>{detail.record.setupDigest}</code></p><p style={{whiteSpace:"pre-wrap",overflowWrap:"anywhere"}}>{detail.record.message || "原记录没有附加说明。"}</p><p>已确认决定 {detail.record.acknowledgedDecisions}</p><details><summary>查看原记录字段</summary><pre style={{whiteSpace:"pre-wrap",overflowWrap:"anywhere"}}>{JSON.stringify(detail.record,null,2)}</pre></details><button onClick={() => { detailGeneration.current++; setDetail(null); }}>关闭</button></section>}
  </section>;
}

