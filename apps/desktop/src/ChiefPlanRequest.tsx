import { useEffect, useRef, useState } from "react";
import { ErrorText } from "./ErrorText";
import type { Command, SavedProjectSetup, Snapshot } from "./contracts";

type Attempt = {
  state: "submitting" | "accepted" | "unconfirmed" | "checked";
  previousOperation?: string;
  startedAt: number;
  error?: string;
};

export function ChiefPlanRequest({ data, saved, projectId, issue, busy, send }: {
  data: Snapshot;
  saved?: SavedProjectSetup;
  projectId: string;
  issue?: string;
  busy: boolean;
  send: (command: Command, onError?: (message: string) => void) => Promise<boolean>;
}) {
  const [attempt, setAttempt] = useState<Attempt>();
  const [reading, setReading] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const inFlight = useRef(false);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const latest = data.managementDecisions.find(d => d.projectId === projectId && d.purpose === "plan");
  const current = attempt && latest?.operationId === attempt.previousOperation ? undefined : latest;
  const count = data.managementHistory.projects.find(p => p.projectId === projectId)?.total ?? 0;
  const submitting = attempt?.state === "submitting";
  const needsRead = !!attempt && !current && attempt.state !== "checked";
  const active = submitting || current?.state === "running" || current?.state === "awaitingExternal";
  useEffect(() => {
    if (!active || !attempt) return;
    const update = () => setElapsed(Math.floor((Date.now() - attempt.startedAt) / 1000));
    update();
    const timer = window.setInterval(update, 1000);
    return () => window.clearInterval(timer);
  }, [active, attempt?.startedAt]);

  async function request() {
    if (!saved || issue || needsRead || inFlight.current) return;
    inFlight.current = true;
    const started: Attempt = { state: "submitting", previousOperation: latest?.operationId, startedAt: Date.now() };
    setAttempt(started);
    setElapsed(0);
    let error: string | undefined;
    try {
      const accepted = await send({ type: "requestChiefPlan", projectId, setupDigest: saved.digest }, message => { error = message; });
      if (mounted.current) setAttempt({ ...started, state: accepted ? "accepted" : "unconfirmed",
        error: error || (accepted ? undefined : "请求尚未确认，请读取原请求记录核对。") });
    } catch (cause) {
      if (mounted.current) setAttempt({ ...started, state: "unconfirmed", error: String(cause) });
    } finally { inFlight.current = false; }
  }
  async function read() {
    if (busy || inFlight.current) return;
    inFlight.current = true;
    setReading(true);
    let error: string | undefined;
    try {
      const confirmed = await send({ type: "snapshot" }, message => { error = message; });
      if (mounted.current) setAttempt(previous => previous && ({ ...previous,
        state: confirmed ? "checked" : previous.state, error: error || previous.error }));
    } catch (cause) {
      if (mounted.current) setAttempt(previous => previous && ({ ...previous, error: String(cause) }));
    } finally { inFlight.current = false; if (mounted.current) setReading(false); }
  }

  let stage = 0;
  let label = issue || "配置已保存，可以请求主指挥生成方案。";
  if (submitting) label = "正在提交规划请求，等待核心确认…";
  else if (current) {
    stage = 1;
    switch (current.state) {
      case "awaitingExternal": label = "请求已保存，等待绑定的 Codex 聊天读取材料并回填方案。"; break;
      case "running": label = "主指挥正在生成方案，工作台持续读取进度。"; break;
      case "ready": case "applying": stage = 2; label = "已收到主指挥回复，正在核验方案。"; break;
      case "applied": {
        const revision = data.workflows.find(w => w.project.id === projectId)?.revision;
        const complete = !current.hasWait && !current.message && !!revision
          && (!revision.managementSetupDigest || revision.managementSetupDigest === saved?.digest);
        stage = complete ? 3 : 2;
        label = complete ? "方案已生成，请审阅方案后决定是否批准。" : "主指挥回复已记录，请核对补充问题或处理结果。";
        break;
      }
      case "unknown": label = "本次请求结果待核对，请读取原记录。"; break;
      default: label = "该次请求已结束，可在主副指挥请求记录中查看处理结果。";
    }
  } else if (attempt?.state === "accepted") label = "核心已接收请求，等待读取本次规划记录。";
  else if (attempt) label = "规划请求需要处理，请核对下方原因及原记录。";
  const error = current?.message || (!current ? attempt?.error : undefined);
  return <section className="planning-request-actions" aria-label="主指挥规划请求">
    <div className="planning-request-heading">
      <div><strong>生成项目方案</strong><p className="secondary">历史请求 {count}{saved ? ` / ${saved.setup.managementMaxRequests}` : ""} 次，包含已放弃记录</p></div>
      <button type="button" className="primary" disabled={!!issue || needsRead || submitting}
        aria-describedby="planning-request-issue"
        onClick={() => void request()}>{submitting ? "正在提交…" : "请求主指挥生成方案"}</button>
    </div>
    <div className="planning-request-progress" aria-busy={!!active}>
      <div className="planning-progress-caption"><p id="planning-request-issue" role="status">{label}</p>{active && attempt && <span className="secondary">已等待 {elapsed} 秒</span>}</div>
      <progress aria-label="主指挥规划进度" aria-valuetext={label} max={3} value={submitting ? undefined : stage}/>
      <ol className="planning-progress-stages" aria-label="规划阶段">
        {["提交请求", "主指挥规划", "方案待审阅"].map((text, index) => <li key={text} className={stage > index ? "completed" : ""}
          aria-current={stage === index ? "step" : undefined}>{text}</li>)}
      </ol>
    </div>
    {issue && issue !== label && !busy && !active && <p className="secondary" role="status">{issue}</p>}
    {error && <div role="alert" className="planning-request-error"><ErrorText error={error}/></div>}
    {(attempt || current) && <div className="planning-request-footer">
      <button type="button" className="secondary-button" disabled={busy || submitting || reading} onClick={() => void read()}>{reading ? "正在读取…" : "读取原请求记录"}</button>
      {current?.state === "awaitingExternal" && <span className="secondary">请在绑定的聊天中处理原规划请求并回填结果。</span>}
    </div>}
  </section>;
}
