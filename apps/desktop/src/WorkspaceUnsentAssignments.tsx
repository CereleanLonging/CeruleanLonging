import { HelpHeading } from "./HelpHeading";
import { useEffect, useRef, useState } from "react";
import type { UnsentAssignmentReview, WorkspaceUnsentAssignmentDetail, WorkspaceUnsentAssignmentPage } from "./contracts";
import type { WorkspaceReader } from "./WorkspaceBrowser";

type Props = { projectId: string; read: WorkspaceReader; busy: boolean };
const PAGE_LIMIT = 20;
const PAGE_BYTES = 64 * 1024;
const DETAIL_BYTES = 512 * 1024;

function validRecord(record: UnsentAssignmentReview, projectId: string) {
  return !!record && record.projectId === projectId && [record.taskId, record.attemptId, record.evidenceDigest].every(value => typeof value === "string" && !!value);
}
function sameRecord(a: UnsentAssignmentReview, b: UnsentAssignmentReview) {
  return a.projectId === b.projectId && a.taskId === b.taskId && a.attemptId === b.attemptId && a.evidenceDigest === b.evidenceDigest;
}
function validPage(value: WorkspaceUnsentAssignmentPage, projectId: string, offset: number, revision: string | null) {
  if (!value || !Array.isArray(value.entries) || !value.entries.every(entry => validRecord(entry, projectId))) return false;
  const end = offset + value.entries.length;
  return !!value && value.projectId === projectId && typeof value.revision === "string" && !!value.revision
    && (revision === null || value.revision === revision) && value.offset === offset && Number.isSafeInteger(offset) && offset >= 0
    && Number.isSafeInteger(value.total) && value.total >= 0 && value.total >= offset && value.entries.length <= PAGE_LIMIT && end <= value.total
    && (value.total === 0 ? value.entries.length === 0 && value.nextOffset === null : value.entries.length > 0)
    && (end === value.total ? value.nextOffset === null : value.nextOffset === end)
    && (value.unavailableReason === null || (typeof value.unavailableReason === "string" && !!value.unavailableReason.trim()))
    && (value.unavailableReason === null || value.total === 0)
    && new Set(value.entries.map(entry => JSON.stringify([entry.taskId, entry.attemptId]))).size === value.entries.length
}
function Fields({ record }: { record: UnsentAssignmentReview }) {
  return <p>原项目 <code>{record.projectId}</code> · 原任务 <code>{record.taskId}</code> · 原尝试 <code>{record.attemptId}</code> · 原证据摘要 <code>{record.evidenceDigest}</code></p>;
}

export function WorkspaceUnsentAssignments(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(true), lock = useRef(false), generation = useRef(0), detailGeneration = useRef(0);
  const shown = useRef(new Map<number, WorkspaceUnsentAssignmentPage>());
  const [page, setPage] = useState<WorkspaceUnsentAssignmentPage | null>(null), [back, setBack] = useState<number[]>([]);
  const [detail, setDetail] = useState<WorkspaceUnsentAssignmentDetail | null>(null);
  const [loading, setLoading] = useState(false), [stale, setStale] = useState(false), [error, setError] = useState("");
  const blocked = loading || props.busy;
  async function run(action: (current: () => boolean) => Promise<void>) {
    if (lock.current || latest.current.busy) return;
    lock.current = true; const token = ++generation.current, projectId = latest.current.projectId;
    const current = () => alive.current && generation.current === token && latest.current.projectId === projectId;
    setLoading(true); setError("");
    try { await action(current); } catch (e) { if (current()) { setError(String(e)); setStale(true); } }
    finally { if (current()) { lock.current = false; setLoading(false); } }
  }
  async function readPage(offset: number, revision: string | null, history: number[], current: () => boolean) {
    const projectId = latest.current.projectId;
    const reply = await latest.current.read({ type: "unsentAssignments", projectId, offset, revision });
    if (!current()) return;
    if (new TextEncoder().encode(JSON.stringify(reply)).byteLength > PAGE_BYTES) throw Error("原未发送分配页超过有界读取大小，请刷新。");
    if (reply.type !== "unsentAssignments" || !validPage(reply.value, projectId, offset, revision)) throw Error("原未发送分配页身份、游标或版本不符，请刷新。");
    const previous = shown.current.get(offset);
    if (previous?.revision === reply.value.revision && (previous.total !== reply.value.total || previous.unavailableReason !== reply.value.unavailableReason || previous.entries.length !== reply.value.entries.length || previous.entries.some((record, i) => !sameRecord(record, reply.value.entries[i])))) throw Error("同一版本的原未发送分配记录不一致，请刷新。");
    const basis = shown.current.values().next().value;
    if (basis?.revision === reply.value.revision) {
      for (const [start, known] of shown.current) {
        if (known.total !== reply.value.total || known.unavailableReason !== reply.value.unavailableReason) throw Error("同一版本的原未发送分配范围不一致，请刷新。");
        if (start !== offset && known.entries.some(record => reply.value.entries.some(entry => record.taskId === entry.taskId && record.attemptId === entry.attemptId))) throw Error("原未发送分配跨页身份重复，请刷新。");
      }
    } else { detailGeneration.current++; setDetail(null); shown.current.clear(); }
    shown.current.set(offset, reply.value); setPage(reply.value); setBack(history); setStale(false);
  }
  useEffect(() => {
    alive.current = true; lock.current = false; generation.current++; detailGeneration.current++; shown.current.clear();
    setPage(null); setDetail(null); setBack([]); setStale(false); setError(""); setLoading(false);
    void run(current => readPage(0, null, [], current));
    return () => { alive.current = false; generation.current++; detailGeneration.current++; lock.current = false; };
  }, [props.projectId]);
  useEffect(() => { if (!props.busy && !page && !loading && !stale) void run(current => readPage(0, null, [], current)); }, [props.busy, props.projectId, page, loading, stale]);
  function readDetail(record: UnsentAssignmentReview) {
    if (!page || stale || lock.current || latest.current.busy) return;
    const projectId = props.projectId, revision = page.revision, token = ++detailGeneration.current;
    void run(async current => {
      try {
        const reply = await latest.current.read({ type: "unsentAssignment", projectId, taskId: record.taskId, attemptId: record.attemptId, revision });
        if (!current() || token !== detailGeneration.current) return;
        if (new TextEncoder().encode(JSON.stringify(reply)).byteLength > DETAIL_BYTES) throw Error("原未发送分配详情超过有界读取大小，请刷新。");
        if (reply.type !== "unsentAssignment" || reply.value.projectId !== projectId || reply.value.revision !== revision || !validRecord(reply.value.record, projectId) || !sameRecord(record, reply.value.record)) throw Error("原未发送分配详情身份、版本或原记录不符，请刷新。");
        setDetail(reply.value);
      } catch (e) { if (current() && token === detailGeneration.current) throw e; }
    });
  }
  return <section className="panel" aria-label="项目未发送分配审阅" aria-busy={loading}>
    <div className="section-heading"><HelpHeading as="h3" topic="WorkspaceUnsentAssignments">项目 {props.projectId} 的未发送分配审阅</HelpHeading><button disabled={blocked} onClick={() => void run(current => readPage(0, null, [], current))}>刷新未发送分配审阅</button></div>
    <p>这里只读原始未发送分配审阅；不会发送、放弃或批准任务，也不会启动模型任务。</p>
    <p>原证据摘要对应无 DispatchIntent 的原始审阅证据，不是发送许可；本页没有放弃、发送或批准操作。</p>
    {loading && <p role="status">正在读取原未发送分配审阅…</p>}{error && <p role="alert">{error} 上次内容仅供核对，请刷新后继续。</p>}
    {!page ? <p>未发送分配审阅尚未取得；这不表示没有原记录。</p> : <>
      <p>原未发送分配审阅版本 <code>{page.revision}</code></p>
      {page.unavailableReason !== null ? <p role="alert">原未发送分配审阅暂不可用：{page.unavailableReason}；这不表示没有原记录。</p> : page.total === 0 ? <p>原项目当前没有未发送分配审阅记录。</p> : page.entries.map(record => <article className="panel" key={JSON.stringify([record.taskId, record.attemptId])}><HelpHeading as="h4" topic="WorkspaceUnsentAssignments">原未发送分配</HelpHeading><Fields record={record}/><button disabled={blocked || stale} onClick={() => readDetail(record)}>查看完整原未发送分配</button></article>)}
      {page.total > 0 && <nav aria-label="未发送分配分页"><button disabled={blocked || stale || !back.length} onClick={() => { const next = back.at(-1)!; void run(current => readPage(next, page.revision, back.slice(0, -1), current)); }}>上一页</button><span>当前显示 {page.offset + page.entries.length} / {page.total}</span><button disabled={blocked || stale || page.nextOffset === null} onClick={() => void run(current => readPage(page.nextOffset!, page.revision, [...back, page.offset], current))}>下一页</button></nav>}
    </>}
    {detail && <section className="panel" role="dialog" aria-label="完整原未发送分配审阅"><HelpHeading as="h4" topic="WorkspaceUnsentAssignments">完整原未发送分配审阅</HelpHeading><Fields record={detail.record}/><details><summary>查看原记录字段</summary><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{JSON.stringify(detail.record, null, 2)}</pre></details><button onClick={() => { detailGeneration.current++; setDetail(null); }}>关闭</button></section>}
  </section>;
}
