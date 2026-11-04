import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { useEffect, useRef, useState } from "react";
import type { Command, RecheckDispositionReview, RecheckStatus, VerificationState, WorkspaceRecheckPage, WorkspaceRecheckSummary } from "./contracts";
import type { WorkspaceReader } from "./WorkspaceBrowser";
import { WorkspaceVerificationResult } from "./WorkspaceVerificationResult";

type Props = { projectId: string; read: WorkspaceReader; send: (command: Command) => Promise<boolean>; busy: boolean };
const states: Record<VerificationState, string> = { running: "复验进行中", cancelling: "正在请求停止", succeeded: "复验检查通过，仍待主指挥验收", failed: "复验未通过", unknown: "复验结果待核对，原批准仍保留", abandoned: "复验已结束" };
const hash = (s: string) => /^[a-f0-9]{64}$/.test(s);
export function recheckSummary(s: RecheckStatus, project: string): WorkspaceRecheckSummary {
  const a = s.approval, r = a.review, j = s.job;
  if (a.schemaVersion !== 1 || r.schemaVersion !== 1 || r.projectId !== project || !a.operationId || a.operationId === r.originalOperationId || !r.originalOperationId || !r.taskId || !r.attemptId
    || !hash(s.reviewDigest) || !hash(s.approvalEvent.hash) || !Number.isSafeInteger(s.approvalEvent.sequence) || s.approvalEvent.sequence <= 0
    || a.originalJob.projectId !== project || a.originalJob.operationId !== r.originalOperationId || a.originalJob.taskId !== r.taskId || a.originalJob.attemptId !== r.attemptId || a.originalJob.state !== "failed"
    || (j && (j.projectId !== project || j.operationId !== a.operationId || j.taskId !== r.taskId || j.attemptId !== r.attemptId || !Object.hasOwn(states, j.state)))) throw Error("当前复验批准或作业身份不符，请保留记录核对。");
  if (s.disposition) {
    const d = s.disposition;
    validDisposition(d.review, s, project);
    if (!d.eventOperationId || d.eventOperationId === a.operationId || !hash(d.event.hash) || !Number.isSafeInteger(d.event.sequence) || d.event.sequence <= s.approvalEvent.sequence || typeof d.evidenceUnchanged !== "boolean") throw Error("原处置回执身份不符。");
  }
  return { operationId: a.operationId, originalOperationId: r.originalOperationId, taskId: r.taskId, attemptId: r.attemptId, approvalEvent: s.approvalEvent, reviewDigest: s.reviewDigest };
}
function validDisposition(r: RecheckDispositionReview, s: RecheckStatus, project: string) {
  const a = s.approval, j = r.launch;
  if (r.schemaVersion !== 1 || r.projectId !== project || r.operationId !== a.operationId || r.taskId !== a.review.taskId || r.attemptId !== a.review.attemptId
    || r.approval.hash !== s.approvalEvent.hash || r.approval.sequence !== s.approvalEvent.sequence || !r.reason.trim()
    || ![r.approvalDigest, r.ledgerHead, r.evidenceDigest, r.processEvidenceDigest].every(hash)
    || (j && (j.projectId !== project || j.operationId !== a.operationId || j.taskId !== r.taskId || j.attemptId !== r.attemptId || j.state !== "running" || j.updatedAt !== j.startedAt || j.message !== null || j.reconciliationDigest != null))
    || (s.job && (!j || s.job.startedAt !== j.startedAt))) throw Error("处置审阅与原批准或启动身份不符，请保留记录核对。");
}
function validPage(p: WorkspaceRecheckPage, project: string, offset: number, previous: WorkspaceRecheckPage | null, revision: string | null) {
  const end = offset + p.entries.length;
  return p.projectId === project && hash(p.revision) && p.offset === offset && Number.isSafeInteger(p.total) && p.total >= end && p.entries.length <= 20
    && (revision === null || (p.revision === revision && p.total === previous?.total))
    && (p.entries.length > 0 || offset === p.total) && (end === p.total ? p.nextOffset === null : p.nextOffset === end)
    && new Set(p.entries.map(e => e.operationId)).size === p.entries.length
    && p.entries.every(e => e.operationId && e.originalOperationId && e.operationId !== e.originalOperationId && e.taskId && e.attemptId && hash(e.reviewDigest) && hash(e.approvalEvent.hash) && Number.isSafeInteger(e.approvalEvent.sequence) && e.approvalEvent.sequence > 0);
}

export function WorkspaceVerificationRechecks(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(false), lock = useRef(false), generation = useRef(0);
  const [page, setPage] = useState<WorkspaceRecheckPage | null>(null), [back, setBack] = useState<number[]>([]);
  const [selected, setSelected] = useState<WorkspaceRecheckSummary | null>(null);
  const [loading, setLoading] = useState(false), [stale, setStale] = useState(false), [error, setError] = useState("");
  async function load(offset = 0, revision: string | null = null, history: number[] = []) {
    if (!alive.current || lock.current || latest.current.busy) return;
    lock.current = true; const token = ++generation.current, project = latest.current.projectId;
    const current = () => alive.current && token === generation.current && project === latest.current.projectId;
    setLoading(true); setError("");
    try {
      const reply = await latest.current.read({ type: "verificationRechecks", projectId: project, offset, revision });
      if (!current()) return;
      if (reply.type !== "verificationRechecks" || !validPage(reply.value, project, offset, page, revision)) throw Error("原复验清单身份、版本或分页不符，请刷新。");
      setPage(reply.value); setBack(history); setStale(false);
    } catch (e) { if (current()) { setError(String(e)); setStale(true); } }
    finally { if (current()) { lock.current = false; setLoading(false); } }
  }
  useEffect(() => {
    alive.current = true; lock.current = false; setPage(null); setSelected(null); setBack([]); setStale(false);
    void load();
    return () => { alive.current = false; generation.current++; lock.current = false; };
  }, [props.projectId]);
  useEffect(() => { if (!page && !loading && !stale && !props.busy) void load(); }, [page, loading, stale, props.busy]);
  const blocked = props.busy || loading;
  return <section className="panel" aria-label="项目复验记录" aria-busy={loading}>
    <div className="section-heading"><HelpHeading as="h3" topic="WorkspaceVerificationRechecks">项目 {props.projectId} 的复验记录</HelpHeading><button disabled={blocked} onClick={() => void load()}>刷新复验清单</button></div>
    <p>这里保留原批准，包括尚未启动的复验。读取记录不会运行检查器或重新派发模型任务。</p>
    {loading && <p role="status">正在读取原批准…</p>}
    {error && <p role="alert">{error} 上次清单仅供核对；已选复验仍可单独刷新。</p>}
    <CurrentRecheck projectId={props.projectId} read={props.read} busy={props.busy} select={setSelected}/>
    {page && <>
      <p>共 {page.total} 条原复验批准</p>
      {page.entries.map(e => <article className="panel" key={e.operationId}><HelpHeading as="h4" topic="WorkspaceVerificationRechecks">复验 {e.operationId}</HelpHeading><p>任务 {e.taskId} · 原尝试 {e.attemptId} · 原失败验收 {e.originalOperationId}</p><button disabled={blocked || stale} onClick={() => setSelected(e)}>审阅复验 {e.operationId}</button></article>)}
      <nav aria-label="复验分页"><button disabled={blocked || stale || !back.length} onClick={() => void load(back.at(-1)!, page.revision, back.slice(0, -1))}>上一页</button><span>当前显示 {page.entries.length} / {page.total}</span><button disabled={blocked || stale || page.nextOffset === null} onClick={() => void load(page.nextOffset!, page.revision, [...back, page.offset])}>下一页</button></nav>
    </>}
    {selected && <VerificationRecheckRecord key={props.projectId + ":" + selected.operationId} {...props} selected={selected} close={() => setSelected(null)}/>}
  </section>;
}

function CurrentRecheck(props: { projectId: string; read: WorkspaceReader; busy: boolean; select: (summary: WorkspaceRecheckSummary) => void }) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(false), lock = useRef(false), generation = useRef(0);
  const [record, setRecord] = useState<WorkspaceRecheckSummary | null>(null), [loaded, setLoaded] = useState(false), [loading, setLoading] = useState(false), [error, setError] = useState("");
  async function load() {
    if (!alive.current || lock.current || latest.current.busy) return;
    lock.current = true; const token = ++generation.current, project = latest.current.projectId;
    const current = () => alive.current && token === generation.current && latest.current.projectId === project;
    setLoading(true); setError("");
    try {
      const reply = await latest.current.read({ type: "currentVerificationRecheck", projectId: project });
      if (!current()) return;
      if (reply.type !== "currentVerificationRecheck") throw Error("未取得当前复验记录。");
      const item = reply.value ? recheckSummary(reply.value, project) : null;
      setRecord(item); setLoaded(true);
    } catch (e) { if (current()) setError(String(e)); }
    finally { if (current()) { lock.current = false; setLoading(false); } }
  }
  useEffect(() => {
    alive.current = true; lock.current = false; setRecord(null); setLoaded(false); void load();
    return () => { alive.current = false; generation.current++; lock.current = false; };
  }, [props.projectId]);
  useEffect(() => { if (!loaded && !loading && !error && !props.busy) void load(); }, [loaded, loading, error, props.busy]);
  return <section className="panel" aria-label="当前复验入口"><HelpHeading as="h4" topic="WorkspaceVerificationRechecks">当前复验</HelpHeading><p>单独查找正在执行或尚待处理的原批准，完整历史暂不可读时也可核对。</p>
    <button className="secondary-button" disabled={props.busy || loading} onClick={() => void load()}>查找当前复验</button>
    {error && <p role="alert">{error} 旧入口仅供核对，请刷新。</p>}
    {loaded && !record && !error && <p>当前没有未决复验批准，已完成记录仍在历史清单中。</p>}
    {record && <button className="secondary-button" disabled={props.busy || loading || !!error} onClick={() => props.select(record)}>审阅当前复验 {record.operationId}</button>}
  </section>;
}

type Action = "start" | "cancel" | "recover";
export function VerificationRecheckRecord(props: Props & { selected: WorkspaceRecheckSummary; close: () => void }) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(false), lock = useRef(false), generation = useRef(0);
  const authority = useRef<string | null>(null), launch = useRef<string | null>(null), attempted = useRef(new Set<Action>()), unresolved = useRef<Action | null>(null);
  const dispositionSent = useRef<{ eventOperationId: string; review: RecheckDispositionReview } | null>(null);
  const originalDisposition = useRef<string | null>(null);
  const [reason, setReason] = useState(""), [dispositionReview, setDispositionReview] = useState<RecheckDispositionReview | null>(null), [dispositionAck, setDispositionAck] = useState(false);
  const [status, setStatus] = useState<RecheckStatus | null>(null), [loading, setLoading] = useState(false), [stale, setStale] = useState(false);
  const [error, setError] = useState(""), [message, setMessage] = useState(""), [acknowledged, setAcknowledged] = useState(false), [details, setDetails] = useState(false);
  async function run(action: (current: () => boolean) => Promise<void>) {
    if (!alive.current || lock.current || latest.current.busy) return;
    lock.current = true; const token = ++generation.current, p = latest.current;
    const current = () => alive.current && token === generation.current && p.projectId === latest.current.projectId && p.selected.operationId === latest.current.selected.operationId;
    setLoading(true); setError("");
    try { await action(current); }
    catch (e) { if (current()) { setError(String(e)); setStale(true); } }
    finally { if (current()) { lock.current = false; setLoading(false); } }
  }
  async function refresh(current: () => boolean) {
    const { projectId, selected: e } = latest.current;
    const reply = await latest.current.read({ type: "verificationRecheck", projectId, operationId: e.operationId });
    if (!current()) return;
    if (reply.type !== "verificationRecheck") throw Error("未取得原复验状态。");
    const s = reply.value, a = s.approval, r = a.review, j = s.job;
    recheckSummary(s, projectId);
    const d = s.disposition;
    const receiptIdentity = d ? JSON.stringify([d.eventOperationId, d.event, d.review]) : null;
    if (originalDisposition.current !== null && originalDisposition.current !== receiptIdentity) throw Error("原处置事件缺失或变化，请保留记录核对。");
    if (d) {
      const sent = dispositionSent.current;
      if (sent && (d.eventOperationId !== sent.eventOperationId || JSON.stringify(d.review) !== JSON.stringify(sent.review))) throw Error("处置回执与本次提交不符，请核对原事件。");
      originalDisposition.current = receiptIdentity;
      setMessage("已找到原处置事件；未重新提交或执行复验。");
    }
    if (a.schemaVersion !== 1 || r.schemaVersion !== 1 || a.operationId !== e.operationId || r.projectId !== projectId || r.originalOperationId !== e.originalOperationId || r.taskId !== e.taskId || r.attemptId !== e.attemptId
      || s.reviewDigest !== e.reviewDigest || s.approvalEvent.hash !== e.approvalEvent.hash || s.approvalEvent.sequence !== e.approvalEvent.sequence
      || a.originalJob.projectId !== projectId || a.originalJob.operationId !== e.originalOperationId || a.originalJob.taskId !== e.taskId || a.originalJob.attemptId !== e.attemptId || a.originalJob.state !== "failed"
      || !r.replacementChecks.length || r.replacementChecks.length > 64
      || r.replacementChecks.some(c => !c.id || !c.program || !c.tool || !hash(c.tool.executable.sha256) || !Number.isSafeInteger(c.timeoutSeconds) || c.timeoutSeconds <= 0)
      || (j && (j.projectId !== projectId || j.operationId !== e.operationId || j.taskId !== e.taskId || j.attemptId !== e.attemptId || !Object.hasOwn(states, j.state)))) throw Error("原复验批准、任务、尝试或作业身份不符，请保留记录核对。");
    const original = JSON.stringify(a);
    if (authority.current !== null && authority.current !== original) throw Error("原批准内容发生变化，请保留记录核对。");
    if (launch.current !== null && (!j || j.startedAt !== launch.current)) throw Error("原启动记录缺失或发生变化，请保留记录核对。");
    if (j) launch.current = j.startedAt;
    authority.current = original;
    const pending = unresolved.current;
    if ((pending === "start" && j) || (pending === "cancel" && j && j.state !== "running") || (pending === "recover" && j && ["succeeded", "failed"].includes(j.state))) {
      unresolved.current = null; setMessage("已读取原操作的后续状态，没有再次提交。");
    }
    setStatus(s); setStale(false); setAcknowledged(false); setDispositionReview(null); setDispositionAck(false);
  }
  useEffect(() => {
    alive.current = true; lock.current = false;
    void run(refresh);
    return () => { alive.current = false; generation.current++; lock.current = false; };
  }, []);
  useEffect(() => { if (!status && !loading && !stale && !props.busy) void run(refresh); }, [status, loading, stale, props.busy]);
  const job = status?.job, blocked = props.busy || loading;
  function allowed(action: Action) {
    if (!status || status.disposition || dispositionSent.current || blocked || stale || !acknowledged || unresolved.current || attempted.current.has(action)) return false;
    if (action === "start") return !job;
    if (action === "cancel") return job?.state === "running";
    return job?.state === "unknown";
  }
  function act(action: Action) {
    if (!allowed(action)) return;
    void run(async current => {
      if (attempted.current.has(action)) return;
      attempted.current.add(action); unresolved.current = action; setAcknowledged(false);
      const type = { start: "startVerificationRecheck", cancel: "cancelVerificationRecheck", recover: "recoverVerificationRecheck" } as const;
      let accepted = false;
      try { accepted = await latest.current.send({ type: type[action], projectId: props.projectId, operationId: props.selected.operationId }); }
      catch { /* Keep the exact original operation; never resend an uncertain command. */ }
      if (!current()) return;
      if (!accepted) { setMessage("操作回执未确认。请刷新原复验状态核对；本页不会重发。原批准继续保留。"); return; }
      unresolved.current = null;
      setMessage(action === "recover" ? "只核对原执行结果，未运行检查器。" : "已收到操作回执，请以原作业状态为准。");
      await refresh(current);
    });
  }
  const canReviewDisposition = !!status && !status.disposition && !dispositionSent.current && !blocked && !stale && !unresolved.current && (!job || job.state === "unknown") && !!reason.trim();
  function reviewDisposition() {
    if (!canReviewDisposition || !status) return;
    void run(async current => {
      setDispositionReview(null); setDispositionAck(false);
      const reply = await latest.current.read({ type: "verificationRecheckDispositionReview", projectId: props.projectId, operationId: props.selected.operationId, reason });
      if (!current()) return;
      if (reply.type !== "verificationRecheckDispositionReview") throw Error("未取得原处置审阅。");
      validDisposition(reply.value, status, props.projectId);
      if (reply.value.reason !== reason) throw Error("处置原因与审阅不符。");
      setDispositionReview(reply.value);
    });
  }
  function confirmDisposition() {
    if (!canReviewDisposition || !dispositionReview || !dispositionAck) return;
    void run(async current => {
      if (dispositionSent.current) return;
      const bytes = crypto.getRandomValues(new Uint8Array(16));
      const eventOperationId = "retire-recheck-" + Array.from(bytes, b => b.toString(16).padStart(2, "0")).join("");
      dispositionSent.current = { eventOperationId, review: dispositionReview };
      setDispositionAck(false); setAcknowledged(false);
      let accepted = false;
      try { accepted = await latest.current.send({ type: "retireVerificationRecheck", review: dispositionReview, eventOperationId }); }
      catch { /* Query the original receipt; never resend an uncertain command. */ }
      if (!current()) return;
      setMessage(accepted ? "已收到处置确认，正在读取原事件。" : "处置回执未确认。请刷新原复验状态找回原事件；本页不会重发。");
      await refresh(current);
    });
  }
  const review = status?.approval.review;
  return <section className="panel" aria-label="原复验审阅" aria-busy={loading}>
    <div className="section-heading"><HelpHeading as="h4" topic="WorkspaceVerificationRechecks">原复验 {props.selected.operationId}</HelpHeading><button onClick={props.close}>关闭复验审阅</button></div>
    <button disabled={blocked} onClick={() => void run(refresh)}>刷新原复验状态</button>
    {error && <p role="alert">{error} 上次内容仅供核对，操作已锁定。</p>}
    {message && <p role="status">{message}</p>}
    {status && review && <>
      <p>{status.disposition ? (status.disposition.evidenceUnchanged ? "原复验已处置，原提交仍未验收" : "原复验已处置，但当前证据需核对；操作已锁定") : job ? states[job.state] : "已批准，尚未读到作业记录"}</p>
      {status.disposition && <><p style={{ whiteSpace: "pre-wrap" }}>处置原因：{status.disposition.review.reason}</p><p>原处置回执：{status.disposition.eventOperationId}</p>{status.disposition.message && <p role="alert"><ErrorText error={status.disposition.message}/></p>}</>}
      <p>任务 {review.taskId} · 原尝试 {review.attemptId} · 原失败验收 {review.originalOperationId}</p>
      <p style={{ whiteSpace: "pre-wrap" }}>修订原因：{review.reason}</p>
      <p>复验只检查原提交的封存产物。原失败记录保留；检查通过后仍须主指挥验收，项目不会自动继续调度。</p>
      <ol>{review.replacementChecks.map(c => <li key={c.id}><strong>{c.name || c.id}</strong> · 超时 {c.timeoutSeconds} 秒<p style={{ overflowWrap: "anywhere" }}>程序：{c.program}</p><ol aria-label={`${c.name || c.id} 的参数`}>{c.args.map((arg, i) => <li key={i}>{arg === "" ? <em>空参数</em> : <pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{arg}</pre>}</li>)}</ol><p>检查器摘要：{c.tool?.executable.sha256 ?? "缺少工具摘要"}</p></li>)}</ol>
      {job?.message && <p style={{ whiteSpace: "pre-wrap" }}>{job.message}</p>}
      <label className="checkbox-label"><input type="checkbox" checked={acknowledged} disabled={blocked || stale || !!unresolved.current || !!status.disposition || !!dispositionSent.current} onChange={e => setAcknowledged(e.target.checked)}/>我已核对原批准、检查器和当前状态</label>
      <p>启动前核心会核对原启动记录，已有记录不能再次执行。启动将执行一次上述本地检查；取消请求不等于已停止。核对原结果不会重新执行；结果未知时原批准仍保留。</p>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}><button className="primary" disabled={!allowed("start")} onClick={() => act("start")}>启动这次复验</button><button className="secondary-button" disabled={!allowed("cancel")} onClick={() => act("cancel")}>请求取消这次复验</button><button className="secondary-button" disabled={!allowed("recover")} onClick={() => act("recover")}>核对原复验结果</button></div>
      {!status.disposition && <section className="panel" aria-label="处置原复验">
        <HelpHeading as="h5" topic="WorkspaceVerificationRechecks">结束这次复验批准</HelpHeading>
        <p>用于不再执行的原批准或已经中断的复验。核心会核查原记录及进程是否结束。处置保留原失败和全部证据，不会把产物标为通过，不会启动新检查；后续复验须重新审阅批准。</p>
        <label>处置原因<textarea value={reason} maxLength={2000} disabled={blocked || !!dispositionSent.current} onChange={e => { setReason(e.target.value); setDispositionReview(null); setDispositionAck(false); }}/></label>
        <button disabled={!canReviewDisposition} onClick={reviewDisposition}>只读审阅处置</button>
        {dispositionReview && <><p style={{ whiteSpace: "pre-wrap" }}>将保留原记录并结束此批准：{dispositionReview.reason}</p><p>{dispositionReview.launch ? "原启动身份已核对，当前未发现仍存活的受管进程。" : "原批准尚未启动，未发现执行记录。"}</p>
          <label className="checkbox-label"><input type="checkbox" checked={dispositionAck} disabled={!canReviewDisposition} onChange={e => setDispositionAck(e.target.checked)}/>我确认结束这次复验批准，保留原失败及证据，提交仍未验收</label>
          <button disabled={!canReviewDisposition || !dispositionAck} onClick={confirmDisposition}>确认处置这次复验</button></>}
        {dispositionSent.current && <p>本页已提交一次。请用“刷新原复验状态”核对原处置回执：{dispositionSent.current.eventOperationId}。</p>}
      </section>}
      {job && <button disabled={blocked || stale} onClick={() => setDetails(v => !v)}>{details ? "关闭复验结果" : "查看复验结果与日志"}</button>}
      <details><summary>原批准与证据摘要</summary><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{JSON.stringify(status, null, 2)}</pre></details>
    </>}
    {details && status?.job && <WorkspaceVerificationResult projectId={props.projectId} operationId={props.selected.operationId} taskId={props.selected.taskId} attemptId={props.selected.attemptId} read={props.read} busy={blocked || stale} close={() => setDetails(false)}/>}
  </section>;
}
