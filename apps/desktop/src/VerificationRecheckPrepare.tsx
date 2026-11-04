import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { useEffect, useRef, useState } from "react";
import type { AcceptanceCheck, Command, RecheckReview, WorkspaceRecheckInputs, WorkspaceRecheckSummary } from "./contracts";
import type { WorkspaceReader } from "./WorkspaceBrowser";
import { recheckSummary, VerificationRecheckRecord } from "./WorkspaceVerificationRechecks";

type Props = { projectId: string; taskId: string; attemptId: string; originalOperationId: string; read: WorkspaceReader; send: (command: Command) => Promise<boolean>; busy: boolean; close: () => void };
const hash = (s: string) => /^[a-f0-9]{64}$/.test(s);
function ids() {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  const suffix = Array.from(bytes, b => b.toString(16).padStart(2, "0")).join("");
  return { approvalOperationId: "approve-recheck-" + suffix, operationId: "recheck-" + suffix };
}

export function VerificationRecheckPrepare(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(false), lock = useRef(false), generation = useRef(0);
  const submitted = useRef<{ approvalOperationId: string; operationId: string; review: RecheckReview } | null>(null);
  const [inputs, setInputs] = useState<WorkspaceRecheckInputs | null>(null), [checks, setChecks] = useState<AcceptanceCheck[]>([]), [tools, setTools] = useState<string[]>([]);
  const [reason, setReason] = useState(""), [review, setReview] = useState<RecheckReview | null>(null), [agreed, setAgreed] = useState(false);
  const [loading, setLoading] = useState(false), [stale, setStale] = useState(false), [error, setError] = useState(""), [message, setMessage] = useState("");
  const [confirmed, setConfirmed] = useState<WorkspaceRecheckSummary | null>(null);
  const blocked = props.busy || loading, sent = submitted.current !== null;
  async function run(action: (current: () => boolean) => Promise<void>) {
    if (!alive.current || lock.current || latest.current.busy) return;
    lock.current = true; const token = ++generation.current, p = latest.current;
    const current = () => alive.current && generation.current === token && p.projectId === latest.current.projectId && p.originalOperationId === latest.current.originalOperationId && p.taskId === latest.current.taskId && p.attemptId === latest.current.attemptId;
    setLoading(true); setError("");
    try { await action(current); }
    catch (e) { if (current()) { setError(String(e)); setStale(true); setReview(null); setAgreed(false); } }
    finally { if (current()) { lock.current = false; setLoading(false); } }
  }
  async function load(current: () => boolean) {
    if (submitted.current) return;
    const p = latest.current;
    const reply = await p.read({ type: "verificationRecheckInputs", projectId: p.projectId, originalOperationId: p.originalOperationId });
    if (!current()) return;
    if (reply.type !== "verificationRecheckInputs") throw Error("未取得原验收配置。");
    const v = reply.value, j = v.originalJob;
    if (j.projectId !== p.projectId || j.operationId !== p.originalOperationId || j.taskId !== p.taskId || j.attemptId !== p.attemptId || j.state !== "failed"
      || !hash(v.revisionDigest) || !hash(v.originalChecksDigest) || !Number.isSafeInteger(v.maxTimeoutSeconds) || v.maxTimeoutSeconds <= 0
      || !v.checks.length || v.checks.length > 64 || new Set(v.checks.map(c => c.id)).size !== v.checks.length
      || v.checks.some(c => !c.id || !c.program || !Number.isSafeInteger(c.timeoutSeconds) || c.timeoutSeconds <= 0 || c.timeoutSeconds > v.maxTimeoutSeconds)
      || v.validators.length > 32 || new Set(v.validators.map(t => t.id)).size !== v.validators.length || v.validators.some(t => !t.id || !t.name || !hash(t.digest))) throw Error("原验收配置、任务身份或工具索引不符，请保留记录核对。");
    setInputs(v); setChecks(v.checks); setTools(v.checks.map(() => "")); setReason(""); setReview(null); setAgreed(false); setStale(false);
  }
  useEffect(() => {
    alive.current = true; lock.current = false; void run(load);
    return () => { alive.current = false; generation.current++; lock.current = false; };
  }, []);
  useEffect(() => { if (!inputs && !loading && !stale && !props.busy && !submitted.current) void run(load); }, [inputs, loading, stale, props.busy]);
  function update(index: number, value: AcceptanceCheck) {
    if (blocked || sent) return;
    setChecks(old => old.map((c, i) => i === index ? value : c)); setReview(null); setAgreed(false);
  }
  function choose(index: number, id: string) {
    if (!inputs || blocked || sent) return;
    setReview(null); setAgreed(false);
    if (!id) {
      update(index, { ...checks[index], program: inputs.checks[index].program, tool: inputs.checks[index].tool });
      setTools(old => old.map((v, i) => i === index ? "" : v)); return;
    }
    const original = inputs.validators.find(t => t.id === id);
    if (!original) return;
    void run(async current => {
      const reply = await latest.current.read({ type: "registeredValidator", id, digest: original.digest });
      if (!current()) return;
      if (reply.type !== "registeredValidator" || reply.value.id !== id || reply.value.name !== original.name || reply.value.registeredBy !== "user" || !reply.value.tool.executable.path || !hash(reply.value.tool.executable.sha256)) throw Error("原登记工具身份或摘要不符，请重新读取配置。");
      const tool = reply.value.tool;
      setChecks(old => old.map((c, i) => i === index ? { ...c, program: tool.executable.path, tool } : c));
      setTools(old => old.map((v, i) => i === index ? id : v));
    });
  }
  const valid = !!inputs && !stale && reason.trim().length > 0 && new TextEncoder().encode(reason).length <= 2000 && !reason.includes("\0")
    && checks.every(c => Number.isSafeInteger(c.timeoutSeconds) && c.timeoutSeconds > 0 && c.timeoutSeconds <= inputs.maxTimeoutSeconds && c.tool && c.args.every(a => !a.includes("\0")))
    && JSON.stringify(checks) !== JSON.stringify(inputs.checks);
  function prepare() {
    if (!valid || blocked || sent || !inputs) return;
    setReview(null); setAgreed(false);
    const request = { projectId: props.projectId, originalOperationId: props.originalOperationId, replacementChecks: checks, reason };
    void run(async current => {
      const reply = await latest.current.read({ type: "verificationRecheckReview", input: request });
      if (!current()) return;
      if (reply.type !== "verificationRecheckReview") throw Error("未取得原复验审阅。");
      const r = reply.value;
      if (r.schemaVersion !== 1 || r.projectId !== props.projectId || r.originalOperationId !== props.originalOperationId || r.taskId !== props.taskId || r.attemptId !== props.attemptId
        || r.revisionDigest !== inputs.revisionDigest || r.originalChecksDigest !== inputs.originalChecksDigest || r.reason !== request.reason
        || JSON.stringify(r.replacementChecks) !== JSON.stringify(request.replacementChecks) || !hash(r.ledgerHead) || !hash(r.manifestDigest) || !hash(r.originalReportDigest)) throw Error("复验审阅与当前修改或原提交不符，不能批准。");
      setReview(r); setStale(false);
    });
  }
  async function reconcile(current: () => boolean) {
    const original = submitted.current;
    if (!original) return;
    const reply = await latest.current.read({ type: "verificationRecheck", projectId: props.projectId, operationId: original.operationId });
    if (!current()) return;
    if (reply.type !== "verificationRecheck" || JSON.stringify(reply.value.approval.review) !== JSON.stringify(original.review)) throw Error("尚未确认原批准内容，请继续保留原操作；不会再次提交。");
    const item = recheckSummary(reply.value, props.projectId);
    if (item.operationId !== original.operationId) throw Error("原批准操作编号不符。");
    setConfirmed(item); setStale(false); setMessage("已确认原批准；没有启动检查器。启动须在原复验面板单独操作。");
  }
  function approve() {
    if (!review || !agreed || blocked || stale || submitted.current || lock.current || !alive.current || latest.current.busy) return;
    const original = { ...ids(), review };
    submitted.current = original; setAgreed(false);
    void run(async current => {
      let accepted = false;
      try { accepted = await latest.current.send({ type: "approveVerificationRecheck", ...original }); }
      catch { /* Exact approval retained; do not retry or generate replacement IDs. */ }
      if (!current()) return;
      setMessage(accepted ? "批准回执已收到，正在核对原批准。" : "批准回执未确认。请读取原批准核对，不会再次提交或启动。");
      if (accepted) await reconcile(current);
    });
  }
  return <section className="panel" aria-label="准备修订检查器复验" aria-busy={loading}>
    <div className="section-heading"><HelpHeading as="h3" topic="VerificationRecheckPrepare">为原提交准备复验</HelpHeading><button className="secondary-button" onClick={props.close}>关闭复验准备</button></div>
    <p>任务 {props.taskId} · 原尝试 {props.attemptId} · 原失败验收 {props.originalOperationId}</p>
    <p>修改检查器不会改写原失败记录或重新派发模型任务。阶段顺序和名称保留，原封存产物由核心核对。</p>
    {error && <p role="alert"><ErrorText error={error}/></p>}{message && <p role="status">{message}</p>}
    {loading && <p role="status">正在读取和核对原证据…</p>}
    {!sent && <button className="secondary-button" disabled={blocked} onClick={() => void run(load)}>重新读取原检查配置</button>}
    {inputs && !confirmed && <>
      <fieldset disabled={blocked || sent || stale}><HelpHeading as="legend" topic="VerificationRecheckPrepare">修订检查配置</HelpHeading>
        {checks.map((c, i) => <section className="panel" key={c.id}><HelpHeading as="h4" topic="VerificationRecheckPrepare">{i + 1}. {c.name || c.id}</HelpHeading>
          <label>检查工具<select aria-label={`阶段 ${i + 1} 的检查工具`} value={tools[i]} onChange={e => choose(i, e.target.value)}><option value="">保留原工具</option>{inputs.validators.map(t => <option key={t.id} value={t.id}>{t.name}</option>)}</select></label>
          <p style={{ overflowWrap: "anywhere" }}>{c.program}</p><p>需要新的检查器时，请先在验收工具入口登记新版本。</p>
          {c.args.map((arg, n) => <div key={n}><label>阶段 {i + 1} 的参数 {n + 1}<textarea aria-label={`阶段 ${i + 1} 的参数 ${n + 1}`} value={arg} onChange={e => update(i, { ...c, args: c.args.map((v, k) => k === n ? e.target.value : v) })}/></label><button className="secondary-button" onClick={() => update(i, { ...c, args: c.args.filter((_, k) => k !== n) })}>删除参数 {n + 1}</button></div>)}
          <button className="secondary-button" onClick={() => update(i, { ...c, args: [...c.args, ""] })}>为阶段 {i + 1} 添加参数</button>
          <label>阶段 {i + 1} 的时限（秒）<input aria-label={`阶段 ${i + 1} 的时限（秒）`} type="number" min={1} max={inputs.maxTimeoutSeconds} value={c.timeoutSeconds} onChange={e => update(i, { ...c, timeoutSeconds: Number(e.target.value) })}/></label>
        </section>)}
        <label>修订原因<textarea aria-label="修订原因" value={reason} onChange={e => { setReason(e.target.value); setReview(null); setAgreed(false); }}/></label>
      </fieldset>
      <p>每个参数单独填写，空参数与多行文本会原样保留。时限不得超过原批准的 {inputs.maxTimeoutSeconds} 秒。</p>
      <button className="primary" disabled={blocked || sent || !valid} onClick={prepare}>生成复验审阅</button>
      {review && <section className="panel" aria-label="待批准复验方案"><HelpHeading as="h4" topic="VerificationRecheckPrepare">审阅这次检查器修改</HelpHeading><p style={{ whiteSpace: "pre-wrap" }}>{review.reason}</p>
        {review.replacementChecks.map((c, i) => <article key={c.id}><HelpHeading as="h5" topic="VerificationRecheckPrepare">{c.name || c.id}</HelpHeading><p>原时限 {inputs.checks[i].timeoutSeconds} 秒 → 修订后 {c.timeoutSeconds} 秒</p><p>原程序：{inputs.checks[i].program}</p><p>修订程序：{c.program}</p><details><summary>对照参数与固定文件摘要</summary><p>原参数</p><pre>{JSON.stringify(inputs.checks[i].args, null, 2)}</pre><p>修订参数</p><pre>{JSON.stringify(c.args, null, 2)}</pre><p>原工具摘要：{inputs.checks[i].tool?.executable.sha256 ?? "无"}</p><p>修订工具摘要：{c.tool?.executable.sha256}</p><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{JSON.stringify(c.tool, null, 2)}</pre></details></article>)}
        <p>批准会暂停该项目调度并登记一次复验。本次批准不会启动检查器；检查通过后仍需主指挥验收。</p>
        <label className="checkbox-label"><input type="checkbox" checked={agreed} disabled={blocked || sent || stale} onChange={e => setAgreed(e.target.checked)}/>我已核对修改，批准这份复验方案</label>
        <button className="primary" disabled={blocked || sent || stale || !agreed} onClick={approve}>仅登记这次复验批准</button>
      </section>}
    </>}
    {sent && !confirmed && <><button className="secondary-button" disabled={blocked} onClick={() => void run(reconcile)}>读取原批准结果</button><details><summary>原操作编号</summary><p>批准：{submitted.current?.approvalOperationId}</p><p>复验：{submitted.current?.operationId}</p></details></>}
    {confirmed && <VerificationRecheckRecord projectId={props.projectId} read={props.read} send={props.send} busy={props.busy} selected={confirmed} close={props.close}/>}
  </section>;
}
