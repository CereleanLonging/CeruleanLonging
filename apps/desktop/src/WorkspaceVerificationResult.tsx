import { HelpHeading } from "./HelpHeading";
import { useEffect, useRef, useState } from "react";
import type { VerificationStageState, VerificationState, WorkspaceResult } from "./contracts";
import type { WorkspaceReader } from "./WorkspaceBrowser";

type Detail = Extract<WorkspaceResult, { type: "verificationResult" }>["value"];
type Artifacts = Extract<WorkspaceResult, { type: "verificationResultArtifacts" }>["value"];
type Stages = Extract<WorkspaceResult, { type: "verificationResultStages" }>["value"];
type Props = { projectId: string; operationId: string; taskId: string; attemptId: string; read: WorkspaceReader; busy: boolean; close: () => void };
const states: Record<VerificationState, string> = { running: "验收进行中", cancelling: "正在停止验收", succeeded: "验收检查通过", failed: "验收未通过", unknown: "验收待对账", abandoned: "本次验收已放弃" };
const stageStates: Record<VerificationStageState, string> = { passed: "检查通过", failed: "检查失败", timedOut: "检查超时", observed: "已有观察，待整体验收确认", pending: "尚无完整观察", notRun: "未运行（前项超时）", unknown: "结果待核对" };
function validPage(page: { offset: number; total: number; nextOffset: number | null; entries: unknown[] }, offset: number, total: number) {
  const end = offset + page.entries.length;
  return page.offset === offset && page.total === total && Number.isSafeInteger(offset) && offset >= 0 && offset <= total && page.entries.length <= 20 && end <= total && (total === 0 || page.entries.length > 0) && (end === total ? page.nextOffset === null : page.nextOffset === end);
}
export function WorkspaceVerificationResult(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(false), lock = useRef(false), generation = useRef(0), shown = useRef<Detail | null>(null);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [artifacts, setArtifacts] = useState<Artifacts | null>(null), [artifactBack, setArtifactBack] = useState<number[]>([]);
  const [stages, setStages] = useState<Stages | null>(null), [stageBack, setStageBack] = useState<number[]>([]);
  const [loading, setLoading] = useState(false), [stale, setStale] = useState(false), [error, setError] = useState("");
  const blocked = loading || props.busy;
  async function run(action: (current: () => boolean) => Promise<void>) {
    if (!alive.current || lock.current || latest.current.busy) return;
    lock.current = true;
    const token = ++generation.current, { projectId, operationId, taskId, attemptId } = latest.current;
    const current = () => alive.current && generation.current === token && latest.current.projectId === projectId && latest.current.operationId === operationId && latest.current.taskId === taskId && latest.current.attemptId === attemptId;
    setLoading(true);
    try { await action(current); if (current()) setError(""); }
    catch (e) { if (current()) { setError(String(e)); setStale(true); } }
    finally { if (current()) { lock.current = false; setLoading(false); } }
  }
  async function readDetail(current: () => boolean) {
    const { projectId, operationId, taskId, attemptId } = latest.current;
    // Active writers change the revision. Poll the latest metadata, never an old revision.
    const reply = await latest.current.read({ type: "verificationResult", projectId, operationId, revision: null });
    if (!current()) return;
    if (reply.type !== "verificationResult" || reply.value.projectId !== projectId || reply.value.operationId !== operationId || reply.value.taskId !== taskId || reply.value.attemptId !== attemptId || !reply.value.revision || !reply.value.revisionDigest || !reply.value.evidenceDigest || !Object.hasOwn(states, reply.value.state) || [reply.value.artifactCount, reply.value.stageCount].some(n => !Number.isSafeInteger(n) || n < 0)) throw Error("原验收详情身份、版本或计数不符，请刷新。");
    if (shown.current?.revision !== reply.value.revision) { setArtifacts(null); setStages(null); setArtifactBack([]); setStageBack([]); }
    shown.current = reply.value; setDetail(reply.value); setStale(false);
  }
  useEffect(() => {
    alive.current = true; lock.current = false; shown.current = null; setDetail(null); setArtifacts(null); setStages(null); setArtifactBack([]); setStageBack([]); setError(""); setStale(false); setLoading(false);
    void run(readDetail);
    return () => { alive.current = false; generation.current++; lock.current = false; };
  }, [props.projectId, props.operationId, props.taskId, props.attemptId]);
  useEffect(() => {
    if (!detail || stale || props.busy || loading || !["running", "cancelling"].includes(detail.state)) return;
    const timer = window.setTimeout(() => void run(readDetail), 2000);
    return () => window.clearTimeout(timer);
  }, [detail, stale, props.busy, loading]);
  useEffect(() => {
    if (!detail && !loading && !stale && !props.busy) void run(readDetail);
  }, [detail, loading, stale, props.busy]);
  function readArtifacts(offset: number, history: number[]) {
    if (!detail || stale) return;
    const { projectId, operationId, revision, artifactCount } = detail;
    void run(async current => {
      const reply = await latest.current.read({ type: "verificationResultArtifacts", projectId, operationId, offset, revision });
      if (!current()) return;
      if (reply.type !== "verificationResultArtifacts" || reply.value.projectId !== projectId || reply.value.operationId !== operationId || reply.value.revision !== revision || !validPage(reply.value, offset, artifactCount)) throw Error("原产物页身份、版本或游标不符，请刷新。");
      setArtifacts(reply.value); setArtifactBack(history);
    });
  }
  function readStages(offset: number, history: number[]) {
    if (!detail || stale) return;
    const { projectId, operationId, revision, stageCount } = detail;
    void run(async current => {
      const reply = await latest.current.read({ type: "verificationResultStages", projectId, operationId, offset, revision });
      if (!current()) return;
      if (reply.type !== "verificationResultStages" || reply.value.projectId !== projectId || reply.value.operationId !== operationId || reply.value.revision !== revision || !validPage(reply.value, offset, stageCount)) throw Error("原阶段页身份、版本或游标不符，请刷新。");
      setStages(reply.value); setStageBack(history);
    });
  }
  function close() { alive.current = false; generation.current++; lock.current = false; props.close(); }
  return <section className="panel" style={{ overflowWrap: "anywhere" }} aria-label="独立验收结果" aria-busy={loading}>
    <div className="section-heading"><HelpHeading as="h3" topic="WorkspaceVerificationResult">独立验收结果</HelpHeading><button onClick={close}>关闭验收结果</button></div>
    <p>项目 {props.projectId} · 任务 {props.taskId} · 尝试 {props.attemptId} · 验收 {props.operationId}</p>
    <p>这里只读取原证据，不启动、重跑、停止、补记或放弃验收。各项观察不等于整体验收通过，未知结果仍锁定原资源，不能据此重发。</p>
    {loading && <p role="status">正在读取原验收记录…</p>}
    {error && <p role="alert">{error} 上次内容仅供核对，请刷新后继续。</p>}
    {!detail ? <p>验收详情尚未取得，不能据此判断没有产物或阶段。</p> : <>
      <p><strong>{states[detail.state]}</strong> · {detail.ledgerRecorded ? "已有关联的验收账本记录" : "尚无本次验收账本终态"}</p>
      <p>观察时间 {detail.observedAt}；产物 {detail.artifactCount} 项 · 阶段 {detail.stageCount} 项</p>
      {detail.message && <p>{detail.message}</p>}
      {detail.recoveryError && <p>原补记诊断：{detail.recoveryError}</p>}
      {detail.recovery && <p>原补记审阅：{detail.recovery.passed ? "检查通过" : "检查未通过"}；仅供核对，本页不会补记。</p>}
      <details><summary>原验收元数据与证据字段</summary><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{JSON.stringify(detail, null, 2)}</pre></details>
      <section aria-label="验收产物"><HelpHeading as="h4" topic="WorkspaceVerificationResult">原产物引用</HelpHeading><button disabled={blocked || stale} onClick={() => readArtifacts(0, [])}>读取产物首页</button>
        {!artifacts ? <p>产物页尚未读取。</p> : <>{artifacts.total === 0 ? <p>原记录没有产物引用。</p> : <ol start={artifacts.offset + 1}>{artifacts.entries.map((a, i) => <li key={artifacts.offset + i}><p><code>{a.path}</code> · {a.bytes} 字节</p><p>SHA-256 <code>{a.sha256}</code></p></li>)}</ol>}
          <nav aria-label="验收产物分页"><button disabled={blocked || stale || !artifactBack.length} onClick={() => readArtifacts(artifactBack.at(-1)!, artifactBack.slice(0, -1))}>上一页</button><span>当前显示 {artifacts.entries.length} / {artifacts.total}</span><button disabled={blocked || stale || artifacts.nextOffset === null} onClick={() => readArtifacts(artifacts.nextOffset!, [...artifactBack, artifacts.offset])}>下一页</button></nav></>}
      </section>
      <section aria-label="验收阶段"><HelpHeading as="h4" topic="WorkspaceVerificationResult">原阶段结果</HelpHeading><button disabled={blocked || stale} onClick={() => readStages(0, [])}>读取阶段首页</button>
        {!stages ? <p>阶段页尚未读取。</p> : <>{stages.total === 0 ? <p>原记录没有阶段结果。</p> : <ol start={stages.offset + 1}>{stages.entries.map((s, i) => <li key={stages.offset + i}><HelpHeading as="h4" topic="WorkspaceVerificationResult">{s.name} · {s.id}</HelpHeading><p>{stageStates[s.state]} · 时限 {s.timeoutSeconds} 秒 · 退出码 {s.exitCode ?? "无完整记录"} · {s.timedOut ? "已超时" : "未记载超时"}</p><p>命令 SHA-256 <code>{s.commandDigest}</code></p>{[s.stdout, s.stderr].map((log, index) => <div key={index}><strong>{index === 0 ? "标准输出" : "标准错误"}</strong>{log ? <><p><code>{log.path}</code> · {log.bytes} 字节</p><p>SHA-256 <code>{log.sha256}</code></p></> : <p>尚无可确认引用</p>}</div>)}</li>)}</ol>}
          <nav aria-label="验收阶段分页"><button disabled={blocked || stale || !stageBack.length} onClick={() => readStages(stageBack.at(-1)!, stageBack.slice(0, -1))}>上一页</button><span>当前显示 {stages.entries.length} / {stages.total}</span><button disabled={blocked || stale || stages.nextOffset === null} onClick={() => readStages(stages.nextOffset!, [...stageBack, stages.offset])}>下一页</button></nav></>}
      </section>
    </>}
    <button className="secondary-button" disabled={blocked} onClick={() => void run(readDetail)}>刷新验收结果</button>
  </section>;
}
