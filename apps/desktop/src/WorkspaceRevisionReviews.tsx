import { useEffect, useRef, useState } from "react";
import type { ProjectRevisionReview, WorkspaceRevisionReviewDetail, WorkspaceRevisionReviewPage } from "./contracts";
import type { WorkspaceReader } from "./WorkspaceBrowser";

type Props = { projectId: string; read: WorkspaceReader; busy: boolean };
const countFields = ["acceptedTasks", "cancelledTasks", "retiredTasks", "pendingTasks", "submittedTasks", "verifiedSubmissions"] as const;
function validRecord(record: ProjectRevisionReview, projectId: string) {
  return !!record && record.projectId === projectId && typeof record.revisionDigest === "string" && !!record.revisionDigest
    && typeof record.evidenceDigest === "string" && !!record.evidenceDigest
    && countFields.every(field => Number.isSafeInteger(record[field]) && record[field] >= 0);
}
function sameRecord(a: ProjectRevisionReview, b: ProjectRevisionReview) {
  return a.projectId === b.projectId && a.revisionDigest === b.revisionDigest && a.evidenceDigest === b.evidenceDigest
    && countFields.every(field => a[field] === b[field]);
}
function validPage(value: WorkspaceRevisionReviewPage, projectId: string, offset: number, revision: string | null) {
  return !!value && value.projectId === projectId && typeof value.revision === "string" && !!value.revision
    && (revision === null || value.revision === revision) && value.offset === offset && offset === 0
    && Number.isSafeInteger(value.total) && (value.total === 0 || value.total === 1)
    && Array.isArray(value.entries) && value.entries.length === value.total && value.nextOffset === null
    && (value.unavailableReason === null || (typeof value.unavailableReason === "string" && !!value.unavailableReason.trim()))
    && (value.unavailableReason === null || value.total === 0)
    && value.entries.every(record => validRecord(record, projectId));
}
function RecordFields({ record }: { record: ProjectRevisionReview }) {
  return <><p>项目 {record.projectId} · 修订摘要 <code>{record.revisionDigest}</code> · 证据摘要 <code>{record.evidenceDigest}</code></p>
    <p>已接受任务 {record.acceptedTasks} · 已取消任务 {record.cancelledTasks} · 已退役任务 {record.retiredTasks} · 待处理任务 {record.pendingTasks} · 已提交任务 {record.submittedTasks} · 已验证提交 {record.verifiedSubmissions}</p></>;
}

export function WorkspaceRevisionReviews(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(true), lock = useRef(false), generation = useRef(0), detailGeneration = useRef(0), shown = useRef<WorkspaceRevisionReviewPage | null>(null);
  const [page, setPage] = useState<WorkspaceRevisionReviewPage | null>(null);
  const [detail, setDetail] = useState<WorkspaceRevisionReviewDetail | null>(null);
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
  async function readPage(current: () => boolean) {
    const projectId = latest.current.projectId;
    const reply = await latest.current.read({ type: "revisionReviews", projectId, offset: 0, revision: null });
    if (!current()) return;
    if (new TextEncoder().encode(JSON.stringify(reply)).byteLength > 64 * 1024) throw Error("原修订审阅页超过有界读取大小，请刷新。");
    if (reply.type !== "revisionReviews" || !validPage(reply.value, projectId, 0, null)) throw Error("原修订审阅页身份、游标或版本不符，请刷新。");
    const previous = shown.current;
    if (previous?.revision === reply.value.revision && (previous.total !== reply.value.total || previous.unavailableReason !== reply.value.unavailableReason || previous.entries.some((record, index) => !reply.value.entries[index] || !sameRecord(record, reply.value.entries[index])))) throw Error("同一修订审阅版本的原记录不一致，请刷新。");
    if (previous?.revision !== reply.value.revision) { detailGeneration.current++; setDetail(null); }
    shown.current = reply.value; setPage(reply.value); setStale(false);
  }
  useEffect(() => {
    alive.current = true; lock.current = false; generation.current++; detailGeneration.current++; shown.current = null;
    setPage(null); setDetail(null); setStale(false); setError(""); setLoading(false);
    void run(readPage);
    return () => { alive.current = false; generation.current++; detailGeneration.current++; lock.current = false; };
  }, [props.projectId]);
  useEffect(() => {
    if (!props.busy && !page && !loading && !stale) void run(readPage);
  }, [props.busy, props.projectId, page, loading, stale]);
  function readDetail(record: ProjectRevisionReview) {
    if (!page || stale || lock.current || latest.current.busy) return;
    const projectId = props.projectId, revision = page.revision, token = ++detailGeneration.current;
    void run(async current => {
      const reply = await latest.current.read({ type: "revisionReview", projectId, revision });
      if (!current() || token !== detailGeneration.current) return;
      if (new TextEncoder().encode(JSON.stringify(reply)).byteLength > 512 * 1024) throw Error("原修订审阅详情超过有界读取大小，请刷新。");
      if (reply.type !== "revisionReview" || reply.value.projectId !== projectId || reply.value.revision !== revision || !validRecord(reply.value.record, projectId) || !sameRecord(record, reply.value.record)) throw Error("原修订审阅详情身份、版本或原计数不符，请刷新。");
      setDetail(reply.value);
    });
  }
  return <section className="panel" aria-label="项目修订审阅" aria-busy={loading}>
    <div className="section-heading"><h3>项目 {props.projectId} 的修订审阅</h3><button disabled={blocked} onClick={() => void run(readPage)}>刷新修订审阅</button></div>
    <p>这里只读原始修订与结果控制审阅候选；不是修订历史，不会申请修订、退役任务、启动或暂停运行。</p>
    {loading && <p role="status">正在读取原修订审阅…</p>}
    {error && <p role="alert">{error} 上次内容仅供核对，请刷新后继续。</p>}
    {!page ? <p>修订审阅尚未取得；这不表示没有可审阅候选。</p> : <>
      <p>原修订审阅版本 <code>{page.revision}</code></p>
      {page.unavailableReason !== null ? <p role="alert">原修订审阅暂不可用：{page.unavailableReason}；这不表示没有可审阅候选。</p> : page.total === 0 ? <p>原项目当前没有可审阅修订候选。</p> : page.entries.map(record => <article className="panel" key={record.projectId}>
        <h4>原项目修订候选</h4><RecordFields record={record}/><button disabled={blocked || stale} onClick={() => readDetail(record)}>查看完整原修订审阅</button>
      </article>)}
    </>}
    {detail && <section className="panel" aria-label="完整原项目修订审阅"><h4>完整原修订审阅</h4><RecordFields record={detail.record}/><details><summary>查看原记录字段</summary><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{JSON.stringify(detail.record, null, 2)}</pre></details><button onClick={() => { detailGeneration.current++; setDetail(null); }}>关闭</button></section>}
  </section>;
}
