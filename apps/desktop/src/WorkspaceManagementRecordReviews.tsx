import { HelpHeading } from "./HelpHeading";
import { useEffect, useRef, useState } from "react";
import type { ManagementRecordReview, WorkspaceManagementRecordReviewDetail, WorkspaceManagementRecordReviewPage } from "./contracts";
import type { WorkspaceReader } from "./WorkspaceBrowser";

type Props = { enabled: boolean; read: WorkspaceReader; busy: boolean };
const PAGE_LIMIT = 20;
const PAGE_BYTES = 64 * 1024;
const DETAIL_BYTES = 512 * 1024;

function validReview(review: ManagementRecordReview) {
  return !!review && [review.relativePath, review.operationId, review.reason, review.at].every(value => typeof value === "string" && !!value.trim()) && Number.isInteger(review.sequence) && review.sequence >= 0 && review.sequence <= 4 && (review.projectId === null || (typeof review.projectId === "string" && !!review.projectId.trim()));
}
function sameReview(a: ManagementRecordReview, b: ManagementRecordReview) {
  return a.relativePath === b.relativePath && a.operationId === b.operationId && a.sequence === b.sequence && a.projectId === b.projectId && a.reason === b.reason && a.at === b.at;
}
function validPage(value: WorkspaceManagementRecordReviewPage, offset: number, revision: string | null) {
  if (!value || !Array.isArray(value.entries) || !value.entries.every(entry => validReview(entry))) return false;
  const end = offset + value.entries.length;
  return !("projectId" in value) && typeof value.revision === "string" && !!value.revision.trim()
    && (revision === null || value.revision === revision) && value.offset === offset && Number.isSafeInteger(offset) && offset >= 0
    && Number.isSafeInteger(value.total) && value.total >= offset && value.entries.length <= PAGE_LIMIT && end <= value.total
    && (value.total === 0 ? value.entries.length === 0 && value.nextOffset === null : value.entries.length > 0)
    && (end === value.total ? value.nextOffset === null : value.nextOffset === end)
    && new Set(value.entries.map(entry => entry.relativePath)).size === value.entries.length;
}
function Fields({ review }: { review: ManagementRecordReview }) {
  return <><p>原相对路径 <code>{review.relativePath}</code> · 原操作 <code>{review.operationId}</code> · 阶段 {review.sequence} · 项目 {review.projectId ?? "原记录未归属项目"}</p><p style={{ whiteSpace: "pre-wrap" }}>{review.reason}</p><p>原复核时间 <time dateTime={review.at}>{review.at}</time></p></>;
}

export function WorkspaceManagementRecordReviews(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(true), lock = useRef(false), generation = useRef(0), detailGeneration = useRef(0);
  const shown = useRef(new Map<number, WorkspaceManagementRecordReviewPage>());
  const [page, setPage] = useState<WorkspaceManagementRecordReviewPage | null>(null), [back, setBack] = useState<number[]>([]);
  const [detail, setDetail] = useState<WorkspaceManagementRecordReviewDetail | null>(null);
  const [loading, setLoading] = useState(false), [stale, setStale] = useState(false), [error, setError] = useState("");
  const blocked = loading || props.busy || !props.enabled;
  async function run(action: (current: () => boolean) => Promise<void>) {
    if (!alive.current || lock.current || latest.current.busy || !latest.current.enabled) return;
    lock.current = true; const token = ++generation.current;
    const current = () => alive.current && generation.current === token && latest.current.enabled;
    setLoading(true); setError("");
    try { await action(current); } catch (e) { if (current()) { setError(String(e)); setStale(true); } }
    finally { if (current()) { lock.current = false; setLoading(false); } }
  }
  async function readPage(offset: number, revision: string | null, history: number[], current: () => boolean) {
    const reply = await latest.current.read({ type: "managementRecordReviews", offset, revision });
    if (!current()) return;
    if (new TextEncoder().encode(JSON.stringify(reply)).byteLength > PAGE_BYTES) throw Error("原管理复核记录页超过有界读取大小，请刷新。");
    if (reply.type !== "managementRecordReviews" || !validPage(reply.value, offset, revision)) throw Error("原管理复核记录页身份、游标或版本不符，请刷新。");
    const previous = shown.current.get(offset);
    if (previous?.revision === reply.value.revision && (previous.total !== reply.value.total || previous.entries.length !== reply.value.entries.length || previous.entries.some((review, i) => !sameReview(review, reply.value.entries[i])))) throw Error("同一版本的原管理复核记录不一致，请刷新。");
    const basis = shown.current.values().next().value;
    if (basis?.revision === reply.value.revision) {
      for (const [start, known] of shown.current) {
        if (known.total !== reply.value.total) throw Error("同一版本的原管理复核记录范围不一致，请刷新。");
        if (start !== offset && known.entries.some(review => reply.value.entries.some(entry => review.relativePath === entry.relativePath))) throw Error("原管理复核记录跨页路径身份重复，请刷新。");
      }
    } else { detailGeneration.current++; setDetail(null); shown.current.clear(); }
    shown.current.set(offset, reply.value); setPage(reply.value); setBack(history); setStale(false);
  }
  useEffect(() => {
    alive.current = true; lock.current = false; generation.current++; detailGeneration.current++; shown.current.clear();
    setPage(null); setDetail(null); setBack([]); setStale(false); setError(""); setLoading(false);
    void run(current => readPage(0, null, [], current));
    return () => { alive.current = false; generation.current++; detailGeneration.current++; lock.current = false; };
  }, [props.enabled]);
  useEffect(() => { if (props.enabled && !props.busy && !page && !loading && !stale) void run(current => readPage(0, null, [], current)); }, [props.busy, props.enabled, page, loading, stale]);
  function readDetail(review: ManagementRecordReview) {
    if (!page || stale || lock.current || latest.current.busy || !latest.current.enabled) return;
    const revision = page.revision, token = ++detailGeneration.current;
    void run(async current => {
      try {
        const reply = await latest.current.read({ type: "managementRecordReview", relativePath: review.relativePath, revision });
        if (!current() || token !== detailGeneration.current) return;
        if (new TextEncoder().encode(JSON.stringify(reply)).byteLength > DETAIL_BYTES) throw Error("原管理复核记录详情超过有界读取大小，请刷新。");
        if (reply.type !== "managementRecordReview" || !reply.value || "projectId" in reply.value || reply.value.revision !== revision || !validReview(reply.value.review) || !sameReview(review, reply.value.review)) throw Error("原管理复核记录详情身份、版本或原记录不符，请刷新。");
        setDetail(reply.value);
      } catch (e) { if (current() && token === detailGeneration.current) throw e; }
    });
  }
  return <section className="panel" style={{ overflowWrap: "anywhere" }} aria-label="全局管理复核记录" aria-busy={loading}>
    <div className="section-heading"><HelpHeading as="h3" topic="WorkspaceManagementRecordReviews">全局管理复核记录</HelpHeading><button disabled={blocked} onClick={() => void run(current => readPage(0, null, [], current))}>刷新管理复核记录</button></div>
    <p>这里只读全局管理残片复核记录；保留原有项目身份，原记录未归属项目时明确显示未归属。复核不会重发请求，不代表执行成功、批准许可或供应商取消；原任务仍须按实际证据处理。</p>
    {loading && <p role="status">正在读取原管理复核记录…</p>}{error && <p role="alert">读取失败：{error} 上次内容仅供核对，请刷新后继续。</p>}
    {stale && <p role="status">原管理复核记录尚未重新确认；保留内容已过期，详情与翻页已暂停。</p>}
    {!page ? <p>管理复核记录尚未取得；这不表示没有原记录。</p> : <>
      <p>原管理复核记录版本 <code>{page.revision}</code></p>
      {page.total === 0 ? <p>当前没有已复核的 管理残片记录。</p> : page.entries.map(review => <article key={review.relativePath}><HelpHeading as="h4" topic="WorkspaceManagementRecordReviews">原管理复核记录</HelpHeading><Fields review={review}/><button disabled={blocked || stale} onClick={() => readDetail(review)}>查看完整原管理复核记录</button></article>)}
      {page.total > 0 && <nav aria-label="管理复核记录分页"><button disabled={blocked || stale || !back.length} onClick={() => { const next = back.at(-1)!; void run(current => readPage(next, page.revision, back.slice(0, -1), current)); }}>上一页</button><span>当前显示 {page.offset + page.entries.length} / {page.total}</span><button disabled={blocked || stale || page.nextOffset === null} onClick={() => void run(current => readPage(page.nextOffset!, page.revision, [...back, page.offset], current))}>下一页</button></nav>}
    </>}
    {detail && <section aria-label="完整原管理复核记录"><HelpHeading as="h4" topic="WorkspaceManagementRecordReviews">完整原管理复核记录</HelpHeading><Fields review={detail.review}/><details><summary>查看原记录字段</summary><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{JSON.stringify(detail.review, null, 2)}</pre></details><button onClick={() => { detailGeneration.current++; setDetail(null); }}>关闭</button></section>}
  </section>;
}
