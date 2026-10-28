import { useEffect, useRef, useState } from "react";
import type { WorkspaceRecoveryDetail, WorkspaceRecoveryKind, WorkspaceRecoveryPage, WorkspaceRecoveryRecord, WorkspaceRecoverySummary } from "./contracts";
import type { WorkspaceReader } from "./WorkspaceBrowser";

const kinds: Record<WorkspaceRecoveryKind, string> = { managementRecord: "管理复核", startupClosure: "关闭复核", startupReservation: "启动复核", ompStop: "OMP 停止诊断" };
function identity(record: WorkspaceRecoveryRecord) {
  switch (record.type) {
    case "managementRecord": return { id: record.value.relativePath, taskId: null, attemptId: null };
    case "startupClosure": return { id: record.value.relativePath, taskId: null, attemptId: record.value.attemptId };
    case "startupReservation": return { id: record.value.executor + ":" + record.value.attemptId, taskId: null, attemptId: record.value.attemptId };
    case "ompStop": return { id: record.value.pendingPath, taskId: record.value.taskId, attemptId: record.value.attemptId };
  }
}
type Props = { projectId: string; read: WorkspaceReader; busy: boolean };
export function WorkspaceRecovery(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(true), lock = useRef(false), generation = useRef(0);
  const [page, setPage] = useState<WorkspaceRecoveryPage | null>(null), [back, setBack] = useState<number[]>([]);
  const [detail, setDetail] = useState<WorkspaceRecoveryDetail | null>(null);
  const [loading, setLoading] = useState(false), [stale, setStale] = useState(false), [error, setError] = useState("");
  const blocked = loading || props.busy;
  async function run(action: (current: () => boolean) => Promise<void>) {
    if (lock.current) return;
    lock.current = true; const token = ++generation.current, projectId = latest.current.projectId;
    const current = () => alive.current && generation.current === token && latest.current.projectId === projectId;
    setLoading(true); setError("");
    try { await action(current); }
    catch (e) { if (current()) { setError(String(e)); setStale(true); } }
    finally { if (current()) { lock.current = false; setLoading(false); } }
  }
  async function readPage(offset: number, revision: string | null, history: number[], current: () => boolean) {
    const projectId = latest.current.projectId;
    const reply = await latest.current.read({ type: "recovery", projectId, offset, revision });
    if (!current()) return;
    if (reply.type !== "recovery" || reply.value.projectId !== projectId || reply.value.offset !== offset || (revision !== null && reply.value.revision !== revision) || reply.value.entries.some(r => r.projectId !== projectId)) throw Error("原项目恢复页身份或版本不符，请刷新。");
    setPage(reply.value); setBack(history); setDetail(null); setStale(false);
  }
  useEffect(() => {
    alive.current = true; lock.current = false; setPage(null); setDetail(null); setBack([]); setStale(false);
    void run(current => readPage(0, null, [], current));
    return () => { alive.current = false; generation.current++; lock.current = false; };
  }, [props.projectId]);
  function readDetail(summary: WorkspaceRecoverySummary) {
    if (!page) return;
    const projectId = props.projectId, revision = page.revision;
    void run(async current => {
      const reply = await latest.current.read({ type: "recoveryDetail", projectId, kind: summary.kind, id: summary.id, revision });
      if (!current()) return;
      if (reply.type !== "recoveryDetail" || reply.value.projectId !== projectId || reply.value.revision !== revision) throw Error("原恢复详情身份或版本不符，请刷新。");
      const record = reply.value.record, original = identity(record);
      const hasMessage = !!(record.type === "ompStop" ? record.value.message : record.value.reason);
      if (record.type !== summary.kind || record.value.projectId !== projectId || original.id !== summary.id || original.taskId !== summary.taskId || original.attemptId !== summary.attemptId || hasMessage !== summary.hasMessage) throw Error("原恢复类型、记录、任务或尝试不符，请刷新。");
      setDetail(reply.value);
    });
  }
  return <section className="panel" aria-label="项目恢复记录">
    <div className="section-heading"><h3>项目 {props.projectId} 的恢复记录</h3><button disabled={blocked} onClick={() => void run(current => readPage(0, null, [], current))}>刷新恢复首页</button></div>
    <p>这里只读取原管理复核、关闭复核、启动复核和 OMP 停止诊断证据。不发送模型请求，不修改文件，不恢复调度。</p>
    <p>复核不代表任务完成或取消；原有修改保留，处置须回到相应入口重新审阅。未绑定项目的 API 写入恢复不在此列表中。</p>
    {error && <p role="alert">{error} 上次内容仅供核对，请刷新后继续。</p>}
    {!page ? <p>恢复记录尚未取得。</p> : <>
      <p>原项目共 {page.total} 条恢复记录</p>
      {page.total === 0 ? <p>原项目尚无上述恢复记录。</p> : page.entries.map(summary => <article className="panel" key={summary.kind + ":" + summary.id}>
        <h4>{kinds[summary.kind]} · {summary.id}</h4><p>任务 {summary.taskId ?? "无关联任务"} · 尝试 {summary.attemptId ?? "无关联尝试"}{summary.hasMessage ? "；有原说明待查看" : ""}</p>
        <button disabled={blocked || stale} onClick={() => readDetail(summary)}>查看 {kinds[summary.kind]} 记录 {summary.id}</button>
      </article>)}
      <nav aria-label="恢复分页"><button disabled={blocked || stale || !back.length} onClick={() => void run(current => readPage(back.at(-1)!, page.revision, back.slice(0, -1), current))}>上一页</button><span>当前显示 {page.entries.length} / {page.total}</span><button disabled={blocked || stale || page.nextOffset === null} onClick={() => void run(current => readPage(page.nextOffset!, page.revision, [...back, page.offset], current))}>下一页</button></nav>
    </>}
    {detail && <section className="panel" aria-label="原恢复详情"><h4>{kinds[detail.record.type]} 的原记录</h4><p style={{whiteSpace:"pre-wrap",overflowWrap:"anywhere"}}>{(detail.record.type === "ompStop" ? detail.record.value.message : detail.record.value.reason) || "原记录没有附加说明。"}</p><p>本页不会执行恢复、确认、取消或重启操作。</p><details><summary>查看原记录字段</summary><pre style={{whiteSpace:"pre-wrap",overflowWrap:"anywhere"}}>{JSON.stringify(detail.record,null,2)}</pre></details></section>}
  </section>;
}
