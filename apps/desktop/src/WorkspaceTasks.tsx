import { useEffect, useRef, useState } from "react";
import type { TaskState, WorkspaceTaskDetail, WorkspaceTaskEvidencePage, WorkspaceTaskPage } from "./contracts";
import type { WorkspaceReader } from "./WorkspaceBrowser";

const labels: Record<TaskState, string> = { pending: "尚未分配", assigned: "已分配", dispatching: "准备派发", running: "执行中", cancelling: "正在取消", unknown: "结果未知", submitted: "已提交", accepted: "已收口", cancelled: "已取消", retired: "已本地结束" };
type Props = { projectId: string; read: WorkspaceReader; busy: boolean };
export function WorkspaceTasks(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(true), locked = useRef(false), generation = useRef(0);
  const [tasks, setTasks] = useState<WorkspaceTaskPage | null>(null), [back, setBack] = useState<number[]>([]);
  const [detail, setDetail] = useState<WorkspaceTaskDetail | null>(null);
  const [evidence, setEvidence] = useState<WorkspaceTaskEvidencePage | null>(null), [evidenceBack, setEvidenceBack] = useState<number[]>([]);
  const [loading, setLoading] = useState(false), [stale, setStale] = useState(false), [error, setError] = useState("");
  const blocked = loading || props.busy;
  async function run(action: (current: () => boolean) => Promise<void>) {
    if (locked.current) return;
    locked.current = true; const token = ++generation.current;
    const current = () => alive.current && generation.current === token;
    setLoading(true); setError("");
    try { await action(current); }
    catch (e) { if (current()) { setError(String(e)); setStale(true); } }
    finally { if (current()) { locked.current = false; setLoading(false); } }
  }
  async function readPage(offset: number, revision: string | null, history: number[], current: () => boolean) {
    const projectId = latest.current.projectId;
    const reply = await latest.current.read({ type: "tasks", projectId, offset, revision });
    if (!current()) return;
    if (reply.type !== "tasks" || reply.value.project.id !== projectId || reply.value.offset !== offset || (revision !== null && reply.value.revision !== revision)) throw Error("原项目任务页身份或版本不符，请刷新。");
    setTasks(reply.value); setBack(history); setDetail(null); setEvidence(null); setEvidenceBack([]); setStale(false);
  }
  useEffect(() => {
    alive.current = true; locked.current = false; setTasks(null); setDetail(null); setEvidence(null); setBack([]); setEvidenceBack([]); setStale(false);
    void run(current => readPage(0, null, [], current));
    return () => { alive.current = false; generation.current++; locked.current = false; };
  }, [props.projectId]);
  function readDetail(taskId: string) {
    if (!tasks) return;
    const revision = tasks.revision;
    void run(async current => {
      const reply = await latest.current.read({ type: "task", projectId: props.projectId, taskId, revision });
      if (!current()) return;
      if (reply.type !== "task" || reply.value.projectId !== props.projectId || reply.value.task.id !== taskId || reply.value.run.id !== taskId || reply.value.revision !== revision) throw Error("原任务详情身份或版本不符，请刷新。");
      setDetail(reply.value); setEvidence(null); setEvidenceBack([]);
    });
  }
  function readEvidence(offset: number, history: number[]) {
    if (!detail) return;
    const { projectId, revision, task } = detail;
    void run(async current => {
      const reply = await latest.current.read({ type: "taskEvidence", projectId, taskId: task.id, offset, revision });
      if (!current()) return;
      if (reply.type !== "taskEvidence" || reply.value.projectId !== projectId || reply.value.taskId !== task.id || reply.value.revision !== revision || reply.value.offset !== offset || reply.value.total !== detail.run.evidenceCount) throw Error("原任务证据页身份、版本或总数不符，请刷新任务。");
      setEvidence(reply.value); setEvidenceBack(history);
    });
  }
  return <section className="panel" aria-label="项目任务记录">
    <div className="section-heading"><h3>项目 {props.projectId} 的任务</h3><button disabled={blocked} onClick={() => void run(current => readPage(0, null, [], current))}>刷新任务首页</button></div>
    <p>读取原任务状态和证据引用；本页不会派发、取消或批准任务。</p>
    {error && <p role="alert">{error} 上次内容仅供核对，请刷新后继续读取。</p>}
    {!tasks ? <p>任务记录尚未取得。</p> : <>
      <p>原项目共 {tasks.total} 项任务 · {tasks.approved ? "当前方案已批准" : "当前方案尚未批准"}</p>
      {tasks.total === 0 ? <p>原项目尚无任务。</p> : tasks.entries.map(task => <article className="panel" key={task.id}>
        <h4>{task.id} · {task.goal}</h4><p>{labels[task.state]} · 执行者 {task.worker ?? "尚未分配"} · 尝试 {task.attempt ?? "尚无"}</p>
        <p>{task.evidenceCount} 条证据引用 · {task.verified ? "独立检查通过" : "尚无通过的独立检查"}</p>
        {task.state === "unknown" && <p>结果未知，仍占用原资源；不能据此重发或认定已取消。</p>}
        <button disabled={blocked || stale} onClick={() => readDetail(task.id)}>查看任务 {task.id}</button>
      </article>)}
      <nav aria-label="任务分页"><button disabled={blocked || stale || !back.length} onClick={() => void run(current => readPage(back.at(-1)!, tasks.revision, back.slice(0, -1), current))}>上一页</button><span>当前显示 {tasks.entries.length} / {tasks.total}</span><button disabled={blocked || stale || tasks.nextOffset === null} onClick={() => void run(current => readPage(tasks.nextOffset!, tasks.revision, [...back, tasks.offset], current))}>下一页</button></nav>
    </>}
    {detail && <section className="panel" aria-label="原任务详情"><h4>任务 {detail.task.id} 的原记录</h4><p>{detail.task.goal}</p><p>状态：{labels[detail.run.state]}；尝试：{detail.run.attempt ?? "尚无"}；执行代次：{detail.run.fence}</p><p>前置任务：{detail.task.dependencies.join("、") || "无"}；资源声明：{detail.task.resourceKeys?.join("、") || "无"}</p><p>取消请求：{detail.run.cancellationRequested === true ? "已请求，不等于已取消" : detail.run.cancellationRequested === false ? "未请求" : "原记录未记载"}</p>
      <button disabled={blocked || stale} onClick={() => readEvidence(0, [])}>读取原证据引用</button>
      {!evidence ? <p>证据引用尚未读取，共 {detail.run.evidenceCount} 条。</p> : <><p>原证据共 {evidence.total} 条；引用存在不等于执行或验收成功。</p><ol start={evidence.offset + 1}>{evidence.entries.map((entry, i) => <li key={evidence.offset + i}><code>{entry}</code></li>)}</ol><nav aria-label="任务证据分页"><button disabled={blocked || stale || !evidenceBack.length} onClick={() => readEvidence(evidenceBack.at(-1)!, evidenceBack.slice(0, -1))}>上一页</button><span>当前显示 {evidence.entries.length} / {evidence.total}</span><button disabled={blocked || stale || evidence.nextOffset === null} onClick={() => readEvidence(evidence.nextOffset!, [...evidenceBack, evidence.offset])}>下一页</button></nav></>}
    </section>}
  </section>;
}
