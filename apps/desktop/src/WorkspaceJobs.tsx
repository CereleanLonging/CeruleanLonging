import { HelpHeading } from "./HelpHeading";
import { useEffect, useRef, useState } from "react";
import type { Command, WorkspaceJobDetail, WorkspaceJobKind, WorkspaceJobPage, WorkspaceJobSummary } from "./contracts";
import type { WorkspaceReader } from "./WorkspaceBrowser";
import { WorkerObservations } from "./WorkerObservations";
import { WorkspaceVerificationResult } from "./WorkspaceVerificationResult";
import { VerificationRecheckPrepare } from "./VerificationRecheckPrepare";

const kinds: Record<WorkspaceJobKind, string> = { codex: "Codex", omp: "OMP", api: "API", verification: "独立验收" };
const states: Record<string, string> = { starting: "准备启动", executing: "执行中", "cancel-requested": "已请求取消", cancelling: "正在取消", cancelled: "已取消", reconciling: "正在核对", submitted: "已提交", unknown: "结果未知", failed: "失败", abandoned: "已放弃", retired: "已本地结束", running: "检查中", succeeded: "检查成功" };
function validCursor(value: { offset: number; total: number; nextOffset: number | null; entries: unknown[] }, offset: number) {
  const end = offset + value.entries.length;
  return value.offset === offset && Number.isSafeInteger(offset) && offset >= 0 && Number.isSafeInteger(value.total) && value.total >= offset && value.entries.length <= 20 && end <= value.total && (value.total === 0 || value.entries.length > 0) && (end === value.total ? value.nextOffset === null : value.nextOffset === end);
}
type Props = { projectId: string; read: WorkspaceReader; send: (command: Command) => Promise<boolean>; busy: boolean; onTaskEvidence: () => void };
export function WorkspaceJobs(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(true), lock = useRef(false), generation = useRef(0);
  const [page, setPage] = useState<WorkspaceJobPage | null>(null), [back, setBack] = useState<number[]>([]);
  const [detail, setDetail] = useState<WorkspaceJobDetail | null>(null);
  const [observation, setObservation] = useState<WorkspaceJobSummary | null>(null);
  const [verification, setVerification] = useState<WorkspaceJobSummary | null>(null);
  const [prepareRecheck, setPrepareRecheck] = useState<WorkspaceJobSummary | null>(null);
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
    const reply = await latest.current.read({ type: "jobs", projectId, offset, revision });
    if (!current()) return;
    if (reply.type !== "jobs" || reply.value.projectId !== projectId || !reply.value.revision || (revision !== null && (reply.value.revision !== revision || reply.value.total !== page?.total)) || !validCursor(reply.value, offset) || reply.value.entries.some(j => j.projectId !== projectId || !j.id || !j.taskId || !j.attemptId || !Object.hasOwn(kinds, j.status.type) || (j.status.type !== "verification" && j.id !== j.attemptId)) || new Set(reply.value.entries.map(j => j.status.type + ":" + j.id)).size !== reply.value.entries.length) throw Error("原项目作业页身份、游标或版本不符，请刷新。");
    setPage(reply.value); setBack(history); setDetail(null); setObservation(null); setStale(false);
    setVerification(null);
    setPrepareRecheck(null);
  }
  useEffect(() => {
    alive.current = true; lock.current = false; setPage(null); setDetail(null); setObservation(null); setBack([]); setStale(false);
    setVerification(null);
    setPrepareRecheck(null);
    void run(current => readPage(0, null, [], current));
    return () => { alive.current = false; generation.current++; lock.current = false; };
  }, [props.projectId]);
  function readDetail(job: WorkspaceJobSummary) {
    if (!page) return;
    const projectId = props.projectId, revision = page.revision, kind = job.status.type;
    void run(async current => {
      const reply = await latest.current.read({ type: "job", projectId, kind, id: job.id, revision });
      if (!current()) return;
      if (reply.type !== "job" || reply.value.projectId !== projectId || reply.value.revision !== revision) throw Error("原作业详情身份或版本不符，请刷新。");
      const r = reply.value.record, id = r.type === "verification" ? r.value.operationId : r.value.attemptId;
      const updatedAt = r.type === "codex" || r.type === "verification" ? r.value.updatedAt : null;
      if (r.type !== kind || id !== job.id || r.value.projectId !== projectId || r.value.taskId !== job.taskId || r.value.attemptId !== job.attemptId || r.value.state !== job.status.value || r.value.startedAt !== job.startedAt || updatedAt !== job.updatedAt || (r.value.message !== null) !== job.hasMessage) throw Error("原作业类型、任务、尝试或摘要不符，请刷新。");
      setDetail(reply.value);
    });
  }
  return <section className="panel" aria-label="项目作业记录" aria-busy={loading}>
    <div className="section-heading"><HelpHeading as="h3" topic="WorkspaceJobs">项目 {props.projectId} 的作业</HelpHeading><button disabled={blocked} onClick={() => void run(current => readPage(0, null, [], current))}>刷新作业首页</button></div>
    <p>Codex、OMP、API 和独立验收的原作业记录。桌面会话任务仍在原入口查看。</p>
    <p>这里只读取；取消请求、已提交和检查成功都不等于项目收口。资源占用请核对项目任务状态，未知结果不能据此重发。</p>
    {loading && <p role="status">正在读取原作业记录…</p>}
    {error && <p role="alert">{error} 上次内容仅供核对，请刷新后继续。</p>}
    {!page ? <p>作业记录尚未取得。</p> : <>
      <p>原项目共 {page.total} 条作业记录</p>
      {page.total === 0 ? <p>原项目尚无上述作业。</p> : page.entries.map(job => <article className="panel" key={job.status.type + ":" + job.id}>
        <HelpHeading as="h4" topic="WorkspaceJobs">{kinds[job.status.type]} · {job.id}</HelpHeading><p>任务 {job.taskId} · 尝试 {job.attemptId} · {states[job.status.value] ?? job.status.value}</p>
        <p>开始于 {job.startedAt}{job.updatedAt ? `；更新于 ${job.updatedAt}` : ""}{job.hasMessage ? "；有原说明待查看" : ""}</p>
        <button disabled={blocked || stale} onClick={() => readDetail(job)}>查看 {kinds[job.status.type]} 作业 {job.id}</button>
        {job.status.type === "codex" && <button className="secondary-button" disabled={blocked || stale} onClick={() => setObservation(job)}>查看尝试 {job.attemptId} 的执行观察</button>}
        {job.status.type === "verification" && <button className="secondary-button" disabled={blocked || stale} onClick={() => setVerification(job)}>查看验收 {job.id} 的结果</button>}
        {job.status.type === "verification" && job.status.value === "failed" && <button className="secondary-button" disabled={blocked || stale} onClick={() => setPrepareRecheck(job)}>为验收 {job.id} 修订检查器</button>}
      </article>)}
      <nav aria-label="作业分页"><button disabled={blocked || stale || !back.length} onClick={() => void run(current => readPage(back.at(-1)!, page.revision, back.slice(0, -1), current))}>上一页</button><span>当前显示 {page.entries.length} / {page.total}</span><button disabled={blocked || stale || page.nextOffset === null} onClick={() => void run(current => readPage(page.nextOffset!, page.revision, [...back, page.offset], current))}>下一页</button></nav>
    </>}
    {detail && <section className="panel" aria-label="原作业详情"><HelpHeading as="h4" topic="WorkspaceJobs">{kinds[detail.record.type]} 的原作业</HelpHeading><p>任务 {detail.record.value.taskId} · 尝试 {detail.record.value.attemptId} · {states[detail.record.value.state] ?? detail.record.value.state}</p><p style={{whiteSpace:"pre-wrap",overflowWrap:"anywhere"}}>{detail.record.value.message ?? "原记录没有附加说明。"}</p><p>恢复或结束操作须回到相应入口重新审阅；本页不会执行这些操作。</p><p>这里是完整原作业记录。任务证据引用在项目任务入口单独分页读取；请核对任务 {detail.record.value.taskId} 和原尝试，引用存在不等于独立验收通过。</p><button disabled={blocked || stale} onClick={props.onTaskEvidence}>查看项目 {detail.projectId} 的任务与证据引用</button><details><summary>查看原记录字段</summary><pre style={{whiteSpace:"pre-wrap",overflowWrap:"anywhere"}}>{JSON.stringify(detail.record,null,2)}</pre></details></section>}
    {observation && observation.projectId === props.projectId && <WorkerObservations key={observation.projectId + ":" + observation.attemptId} projectId={observation.projectId} taskId={observation.taskId} attemptId={observation.attemptId} read={props.read} busy={blocked || stale} close={() => setObservation(null)}/>}
    {verification && verification.projectId === props.projectId && <WorkspaceVerificationResult key={verification.projectId + ":" + verification.id} projectId={verification.projectId} operationId={verification.id} taskId={verification.taskId} attemptId={verification.attemptId} read={props.read} busy={blocked || stale} close={() => setVerification(null)}/>}
    {prepareRecheck && prepareRecheck.projectId === props.projectId && <VerificationRecheckPrepare key={prepareRecheck.projectId + ":" + prepareRecheck.id} projectId={prepareRecheck.projectId} originalOperationId={prepareRecheck.id} taskId={prepareRecheck.taskId} attemptId={prepareRecheck.attemptId} read={props.read} send={props.send} busy={blocked || stale} close={() => setPrepareRecheck(null)}/>}
  </section>;
}
