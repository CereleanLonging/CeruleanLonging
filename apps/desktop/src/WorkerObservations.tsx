import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { useEffect, useRef, useState } from "react";
import type { WorkspaceObservationDetail, WorkspaceObservationEventPage, WorkspaceObservationIncidentPage, WorkspaceQuery, WorkspaceResult } from "./contracts";

type Props = { projectId: string; taskId: string; attemptId: string; read: (query: WorkspaceQuery) => Promise<WorkspaceResult>; busy: boolean; close: () => void };
function validPage(value: { offset: number; total: number; nextOffset: number | null; entries: unknown[] }, offset: number, total: number) {
  const end = offset + value.entries.length;
  return value.offset === offset && value.total === total && Number.isSafeInteger(offset) && offset >= 0 && offset <= total && value.entries.length <= 20 && end <= total && (total === 0 || value.entries.length > 0) && (end === total ? value.nextOffset === null : value.nextOffset === end);
}
export function WorkerObservations(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(false), lock = useRef(false), generation = useRef(0);
  const [detail, setDetail] = useState<WorkspaceObservationDetail | null>(null);
  const [events, setEvents] = useState<WorkspaceObservationEventPage | null>(null), [eventBack, setEventBack] = useState<number[]>([]);
  const [incidents, setIncidents] = useState<WorkspaceObservationIncidentPage | null>(null), [incidentBack, setIncidentBack] = useState<number[]>([]);
  const [loading, setLoading] = useState(false), [stale, setStale] = useState(false), [error, setError] = useState("");
  const blocked = loading || props.busy;
  async function run(action: (current: () => boolean) => Promise<void>) {
    if (lock.current || latest.current.busy) return;
    lock.current = true;
    const token = ++generation.current, { projectId, taskId, attemptId } = latest.current;
    const current = () => alive.current && generation.current === token && latest.current.projectId === projectId && latest.current.taskId === taskId && latest.current.attemptId === attemptId;
    setLoading(true);
    try { await action(current); if (current()) setError(""); }
    catch (e) { if (current()) { setError(String(e)); setStale(true); } }
    finally { if (current()) { lock.current = false; setLoading(false); } }
  }
  async function readDetail(current: () => boolean) {
    const { projectId, taskId, attemptId } = latest.current;
    const reply = await latest.current.read({ type: "workerObservation", projectId, attemptId, revision: null });
    if (!current()) return;
    if (reply.type !== "workerObservation" || reply.value.projectId !== projectId || reply.value.taskId !== taskId || reply.value.attemptId !== attemptId || !reply.value.revision || !reply.value.revisionDigest || !reply.value.evidenceDigest || [reply.value.eventCount, reply.value.incidentCount, reply.value.dropped, reply.value.rejected, reply.value.duplicates].some(n => !Number.isSafeInteger(n) || n < 0)) throw Error("原观察详情身份、版本或计数不符，请重新读取。");
    setDetail(reply.value); setEvents(null); setIncidents(null); setEventBack([]); setIncidentBack([]); setStale(false);
  }
  useEffect(() => {
    alive.current = true; lock.current = false; setDetail(null); setEvents(null); setIncidents(null); setEventBack([]); setIncidentBack([]); setError(""); setStale(false); setLoading(false);
    void run(readDetail);
    return () => { alive.current = false; generation.current++; lock.current = false; };
  }, [props.projectId, props.taskId, props.attemptId]);
  function readEvents(offset: number, history: number[]) {
    if (!detail || stale) return;
    const { projectId, attemptId, revision, eventCount } = detail;
    void run(async current => {
      const reply = await latest.current.read({ type: "workerObservationEvents", projectId, attemptId, offset, revision });
      if (!current()) return;
      if (reply.type !== "workerObservationEvents" || reply.value.projectId !== projectId || reply.value.attemptId !== attemptId || reply.value.revision !== revision || !validPage(reply.value, offset, eventCount) || new Set(reply.value.entries.map(e => e.sequence)).size !== reply.value.entries.length || reply.value.entries.some(e => !Number.isSafeInteger(e.sequence) || e.sequence < 0 || !e.itemDigest || !["command", "fileChange"].includes(e.kind) || !["completed", "failed", "declined"].includes(e.status))) throw Error("原工具观察页身份、游标或版本不符，请重新读取。");
      setEvents(reply.value); setEventBack(history);
    });
  }
  function readIncidents(offset: number, history: number[]) {
    if (!detail || stale) return;
    const { projectId, attemptId, revision, incidentCount } = detail;
    void run(async current => {
      const reply = await latest.current.read({ type: "workerObservationIncidents", projectId, attemptId, offset, revision });
      if (!current()) return;
      if (reply.type !== "workerObservationIncidents" || reply.value.projectId !== projectId || reply.value.attemptId !== attemptId || reply.value.revision !== revision || !validPage(reply.value, offset, incidentCount) || new Set(reply.value.entries.map(i => i.id)).size !== reply.value.entries.length || reply.value.entries.some(i => !i.id || !i.evidenceDigest || !i.actionDigest || !Number.isSafeInteger(i.firstSequence) || i.firstSequence < 0 || !Number.isSafeInteger(i.triggerSequence) || i.triggerSequence < i.firstSequence)) throw Error("原监督异常页身份、游标或版本不符，请重新读取。");
      setIncidents(reply.value); setIncidentBack(history);
    });
  }
  function close() { alive.current = false; generation.current++; lock.current = false; props.close(); }
  return <section className="panel" aria-label="执行观察" aria-busy={loading}>
    <div className="section-heading"><HelpHeading as="h3" topic="WorkerObservations">执行观察</HelpHeading><button onClick={close}>关闭观察</button></div>
    <p>任务 {props.taskId} · 尝试 {props.attemptId}</p>
    <p className="secondary">查看执行器报告的工具完成记录。它们不代替独立验收；建议正文、发送状态和原轮次回执可在指挥记录查看。</p>
    <p>这里只读取原记录，不派发、取消或恢复任务。未知结果仍占用原资源，不能据此重发。</p>
    {loading && <p role="status">正在读取原观察记录…</p>}
    {error && <p role="alert">{error} 上次内容仅供核对，请重新读取后继续。</p>}
    {!detail ? <p>观察记录尚未取得，不能据此判断没有工具事件或监督异常。</p> : <>
      <p>{detail.turnId ? "已关联原执行轮次" : "尚无已确认的执行轮次"} · {detail.finished ? "观察采集已结束" : "观察尚未结束，或中断后未取得结束记录"}</p>
      <p>仅覆盖命令执行和文件变更的完成事件，不代表全部执行活动。没有记录不能据此判定卡循环。</p>
      <p>遗漏 {detail.dropped} 项 · 拒绝 {detail.rejected} 项异常或错轮次事件 · 去重 {detail.duplicates} 项</p>
      {(detail.dropped > 0 || detail.rejected > 0) && <p role="status">观察证据不完整，不可据此自动纠偏。</p>}
      {detail.storageError && <p role="alert"><ErrorText error={detail.storageError}/></p>}
      {detail.supervision ? <section aria-label="监督异常"><HelpHeading as="h4" topic="WorkerObservations">监督异常</HelpHeading><p>连续失败阈值 {detail.supervision.repeatedFailureThreshold} 次 · 每次尝试建议上限 {detail.supervision.maxAdvicePerAttempt} 条</p>
        <p>以下仅记录观察到的连续命令失败，不代表已证明卡循环；建议请求及正文请在主副指挥请求记录中查看。</p>
        <button disabled={blocked || stale} onClick={() => readIncidents(0, [])}>读取监督异常首页</button>
        {!incidents ? <p>监督异常尚未读取，共 {detail.incidentCount} 条。</p> : <>{incidents.total === 0 ? <p>尚未观察到达到阈值的连续失败。</p> : <ul>{incidents.entries.map(i => <li key={i.id}><strong>连续命令失败</strong><p>事件 {i.firstSequence}–{i.triggerSequence}</p><details><summary>异常证据</summary><p><code>{i.id}</code></p><p><code>{i.evidenceDigest}</code></p></details></li>)}</ul>}
          <nav aria-label="监督异常分页"><button disabled={blocked || stale || !incidentBack.length} onClick={() => readIncidents(incidentBack.at(-1)!, incidentBack.slice(0, -1))}>上一页</button><span>当前显示 {incidents.entries.length} / {incidents.total}</span><button disabled={blocked || stale || incidents.nextOffset === null} onClick={() => readIncidents(incidents.nextOffset!, [...incidentBack, incidents.offset])}>下一页</button></nav></>}
      </section> : <p>原方案未启用异常监督。</p>}
      <section aria-label="工具完成记录"><HelpHeading as="h4" topic="WorkerObservations">工具完成记录</HelpHeading><button disabled={blocked || stale} onClick={() => readEvents(0, [])}>读取工具观察首页</button>
        {!events ? <p>工具完成记录尚未读取，共 {detail.eventCount} 条。</p> : <>{events.total === 0 ? <p>尚无可展示的工具完成记录。</p> : <ol start={events.offset + 1}>{events.entries.map(e => <li key={e.sequence}><HelpHeading as="h4" topic="WorkerObservations">{e.kind === "command" ? "命令执行" : "文件变更"} · {e.status === "completed" ? "已完成" : e.status === "failed" ? "失败" : "被拒绝"}</HelpHeading><p>{new Date(Number(e.receivedAtMs)).toLocaleString("zh-CN")} · 退出码 {e.exitCode ?? "未提供"}</p><details><summary>事件摘要</summary><p>事件：<code>{e.itemDigest}</code></p>{e.actionDigest && <p>动作：<code>{e.actionDigest}</code></p>}</details></li>)}</ol>}
          <nav aria-label="工具观察分页"><button disabled={blocked || stale || !eventBack.length} onClick={() => readEvents(eventBack.at(-1)!, eventBack.slice(0, -1))}>上一页</button><span>当前显示 {events.entries.length} / {events.total}</span><button disabled={blocked || stale || events.nextOffset === null} onClick={() => readEvents(events.nextOffset!, [...eventBack, events.offset])}>下一页</button></nav></>}
      </section>
      <details><summary>关联证据</summary><p>会话 {detail.threadId ?? "未确认"}</p><p>轮次 {detail.turnId ?? "未确认"}</p><p>方案 <code>{detail.revisionDigest}</code></p><p>记录 <code>{detail.evidenceDigest}</code></p></details>
    </>}
    <button className="secondary-button" disabled={blocked} onClick={() => void run(readDetail)}>重新读取观察</button>
  </section>;
}
