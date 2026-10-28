import { useEffect, useRef, useState } from "react";
import type { CodexCancellationReview, WorkspaceCodexCancellationDetail, WorkspaceCodexCancellationPage } from "./contracts";
import type { WorkspaceReader } from "./WorkspaceBrowser";

type Props = { projectId: string; read: WorkspaceReader; busy: boolean };
const PAGE_LIMIT = 20;
const PAGE_BYTES = 64 * 1024;
const DETAIL_BYTES = 512 * 1024;

function validReview(review: CodexCancellationReview, projectId: string) {
  return !!review && review.projectId === projectId && [review.taskId, review.attemptId, review.evidenceDigest].every(value => typeof value === "string" && !!value.trim());
}
function sameReview(a: CodexCancellationReview, b: CodexCancellationReview) {
  return a.projectId === b.projectId && a.taskId === b.taskId && a.attemptId === b.attemptId && a.evidenceDigest === b.evidenceDigest;
}
function validPage(value: WorkspaceCodexCancellationPage, projectId: string, offset: number, revision: string | null) {
  if (!value || !Array.isArray(value.entries) || !value.entries.every(entry => validReview(entry, projectId))) return false;
  const end = offset + value.entries.length;
  return value.projectId === projectId && typeof value.revision === "string" && !!value.revision.trim()
    && (revision === null || value.revision === revision) && value.offset === offset && Number.isSafeInteger(offset) && offset >= 0
    && Number.isSafeInteger(value.total) && value.total >= offset && value.entries.length <= PAGE_LIMIT && end <= value.total
    && (value.total === 0 ? value.entries.length === 0 && value.nextOffset === null : value.entries.length > 0)
    && (end === value.total ? value.nextOffset === null : value.nextOffset === end)
    && new Set(value.entries.map(entry => entry.attemptId)).size === value.entries.length;
}
function Fields({ review }: { review: CodexCancellationReview }) {
  return <p>原项目 <code>{review.projectId}</code> · 原任务 <code>{review.taskId}</code> · 原尝试 <code>{review.attemptId}</code> · 原证据摘要 <code>{review.evidenceDigest}</code></p>;
}

export function WorkspaceCodexCancellations(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(true), lock = useRef(false), generation = useRef(0), detailGeneration = useRef(0);
  const shown = useRef(new Map<number, WorkspaceCodexCancellationPage>());
  const [page, setPage] = useState<WorkspaceCodexCancellationPage | null>(null), [back, setBack] = useState<number[]>([]);
  const [detail, setDetail] = useState<WorkspaceCodexCancellationDetail | null>(null);
  const [loading, setLoading] = useState(false), [stale, setStale] = useState(false), [error, setError] = useState("");
  const blocked = loading || props.busy;
  async function run(action: (current: () => boolean) => Promise<void>) {
    if (!alive.current || lock.current || latest.current.busy) return;
    lock.current = true; const token = ++generation.current, projectId = latest.current.projectId;
    const current = () => alive.current && generation.current === token && latest.current.projectId === projectId;
    setLoading(true); setError("");
    try { await action(current); } catch (e) { if (current()) { setError(String(e)); setStale(true); } }
    finally { if (current()) { lock.current = false; setLoading(false); } }
  }
  async function readPage(offset: number, revision: string | null, history: number[], current: () => boolean) {
    const projectId = latest.current.projectId;
    const reply = await latest.current.read({ type: "codexCancellations", projectId, offset, revision });
    if (!current()) return;
    if (new TextEncoder().encode(JSON.stringify(reply)).byteLength > PAGE_BYTES) throw Error("原取消审阅页超过有界读取大小，请刷新。");
    if (reply.type !== "codexCancellations" || !validPage(reply.value, projectId, offset, revision)) throw Error("原取消审阅页身份、游标或版本不符，请刷新。");
    const previous = shown.current.get(offset);
    if (previous?.revision === reply.value.revision && (previous.total !== reply.value.total || previous.entries.length !== reply.value.entries.length || previous.entries.some((review, i) => !sameReview(review, reply.value.entries[i])))) throw Error("同一版本的原取消审阅记录不一致，请刷新。");
    const basis = shown.current.values().next().value;
    if (basis?.revision === reply.value.revision) {
      for (const [start, known] of shown.current) {
        if (known.total !== reply.value.total) throw Error("同一版本的原取消审阅范围不一致，请刷新。");
        if (start !== offset && known.entries.some(review => reply.value.entries.some(entry => review.attemptId === entry.attemptId))) throw Error("原取消审阅跨页尝试身份重复，请刷新。");
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
  function readDetail(review: CodexCancellationReview) {
    if (!page || stale || lock.current || latest.current.busy) return;
    const projectId = props.projectId, revision = page.revision, token = ++detailGeneration.current;
    void run(async current => {
      try {
        const reply = await latest.current.read({ type: "codexCancellation", projectId, attemptId: review.attemptId, revision });
        if (!current() || token !== detailGeneration.current) return;
        if (new TextEncoder().encode(JSON.stringify(reply)).byteLength > DETAIL_BYTES) throw Error("原取消审阅详情超过有界读取大小，请刷新。");
        if (reply.type !== "codexCancellation" || !reply.value || reply.value.projectId !== projectId || reply.value.revision !== revision || !validReview(reply.value.review, projectId) || !sameReview(review, reply.value.review)) throw Error("原取消审阅详情身份、版本或原记录不符，请刷新。");
        setDetail(reply.value);
      } catch (e) { if (current() && token === detailGeneration.current) throw e; }
    });
  }
  return <section className="panel" style={{ overflowWrap: "anywhere" }} aria-label="项目 Codex 取消审阅" aria-busy={loading}>
    <div className="section-heading"><h3>项目 {props.projectId} 的 Codex 取消审阅</h3><button disabled={blocked} onClick={() => void run(current => readPage(0, null, [], current))}>刷新取消审阅</button></div>
    <p>这里只读原始 Codex 取消审阅及证据摘要；摘要用于核对原记录，不代表取消或批准许可。</p>
    {loading && <p role="status">正在读取原取消审阅…</p>}{error && <p role="alert">读取失败：{error} 上次内容仅供核对，请刷新后继续。</p>}
    {stale && <p role="status">原取消审阅尚未重新确认；保留内容已过期，详情与翻页已暂停。</p>}
    {!page ? <p>取消审阅尚未取得；这不表示没有原记录。</p> : <>
      <p>原取消审阅版本 <code>{page.revision}</code></p>
      {page.total === 0 ? <p>原项目当前没有 Codex 取消审阅记录。</p> : page.entries.map(review => <article key={review.attemptId}><h4>原 Codex 取消审阅</h4><Fields review={review}/><button disabled={blocked || stale} onClick={() => readDetail(review)}>查看完整原取消审阅</button></article>)}
      {page.total > 0 && <nav aria-label="取消审阅分页"><button disabled={blocked || stale || !back.length} onClick={() => { const next = back.at(-1)!; void run(current => readPage(next, page.revision, back.slice(0, -1), current)); }}>上一页</button><span>当前显示 {page.offset + page.entries.length} / {page.total}</span><button disabled={blocked || stale || page.nextOffset === null} onClick={() => void run(current => readPage(page.nextOffset!, page.revision, [...back, page.offset], current))}>下一页</button></nav>}
    </>}
    {detail && <section aria-label="完整原 Codex 取消审阅"><h4>完整原 Codex 取消审阅</h4><Fields review={detail.review}/><details><summary>查看原记录字段</summary><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{JSON.stringify(detail.review, null, 2)}</pre></details><button onClick={() => { detailGeneration.current++; setDetail(null); }}>关闭</button></section>}
  </section>;
}
