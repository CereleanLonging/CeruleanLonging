import { qoderUpgradeBlock } from "./qoderUpgrade";
import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { useEffect, useRef, useState } from "react";
import type { Command, DesktopWorkerSelection, Snapshot } from "./contracts";

type Props = {
  data: Snapshot;
  projectId: string;
  app: "qoder";
  worker: DesktopWorkerSelection | undefined | null;
  onChange: (worker: DesktopWorkerSelection | undefined) => void;
  idPrefix?: string;
  selectionKey?: string;
  isolationEnabled?: boolean;
  send?: (command: Command, onError?: (message: string) => void) => Promise<boolean>;
  busy?: boolean;
};
const normalize = (id: string) => id.trim().toLowerCase();
const sessionPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function DesktopWorkerFields(props: Props) {
  return <DesktopSessionFields key={JSON.stringify([props.data.dataDirectory, props.projectId, props.idPrefix, props.selectionKey, props.app])} {...props}/>;
}

function DesktopSessionFields({ data, projectId, app, worker, onChange, idPrefix = "planning-desktop", isolationEnabled = false, send, busy = false }: Props) {
  const [sessionId, setSessionId] = useState(worker?.binding.observation.sessionId ?? "");
  const [phase, setPhase] = useState<"idle" | "submitting" | "waiting">("idle");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const requestedId = useRef("");
  const sessionField = useRef<HTMLInputElement>(null);
  const lock = useRef(false);
  const project = data.workflows.find(w => w.project.id === projectId)?.project;
  const disabled = busy || phase !== "idle" || !!data.desktopAccess?.checking;
  const valid = sessionPattern.test(normalize(sessionId));

  useEffect(() => {
    sessionField.current?.setCustomValidity(!valid ? "请填写完整的 Qoder 会话 ID（UUID）"
      : !worker || normalize(worker.binding.observation.sessionId ?? "") !== normalize(sessionId)
        ? "请先测试并绑定此会话" : "");
  }, [valid, sessionId, worker?.binding.id]);

  useEffect(() => {
    if (worker?.binding.observation.sessionId) setSessionId(worker.binding.observation.sessionId);
  }, [worker?.binding.id]);

  useEffect(() => {
    if (!worker || !project) return;
    let active = true;
    async function resolve() {
      let root = project!.root;
      if (isolationEnabled) {
        const bytes = new TextEncoder().encode(JSON.stringify([app, null, null, [app, worker!.modelLabel]]));
        const hash = Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)))
          .map(b => b.toString(16).padStart(2, "0")).join("");
        root = root.replace(/[\\/]+$/, "") + "\\executors\\" + hash;
      }
      if (active && root !== worker!.projectRoot) onChange({ ...worker!, projectRoot: root, workspaceConfirmed: false });
    }
    void resolve();
    return () => { active = false; };
  }, [project?.root, worker?.binding.id, worker?.modelLabel, worker?.projectRoot, isolationEnabled, app]);

  useEffect(() => {
    if (phase !== "waiting" || busy || data.desktopAccess?.checking) return;
    lock.current = false;
    setPhase("idle");
    if (data.desktopAccess?.error) { setError(data.desktopAccess.error); return; }
    const binding = [...(data.desktopAccess?.bindings ?? [])].reverse().find(binding =>
      binding.projectId === projectId && binding.observation.window.app === app
      && normalize(binding.observation.sessionId ?? "") === requestedId.current);
    if (!binding?.observation.modelLabel || !project) {
      setError("会话绑定结果尚未确认，请核对会话 ID 与当前项目后重新读取。");
      return;
    }
    onChange(worker?.binding.id === binding.id ? worker : {
      binding, projectRoot: project.root, modelLabel: binding.observation.modelLabel,
      workspaceConfirmed: false, externalToolsAcknowledged: false,
    });
    setNotice("会话已核对并绑定，模型已自动读取。");
  }, [phase, busy, data.desktopAccess, projectId, project?.root, app, onChange]);

  async function bind() {
    if (disabled || lock.current || !send || !project || !valid) return;
    lock.current = true;
    requestedId.current = normalize(sessionId);
    setError("");
    setNotice("");
    setPhase("submitting");
    try {
      let submissionError = "";
      const accepted = await send({ type: "bindDesktopSession", projectId, sessionId: requestedId.current }, message => {
        submissionError = message;
        setError(message);
      });
      if (accepted && !submissionError) { setPhase("waiting"); return; }
      setError(message => message || "会话绑定未完成，请核对当前窗口后重试。");
    } catch (error) {
      setError(String(error));
    }
    lock.current = false;
    setPhase("idle");
  }

  return <section className="panel desktop-session-binding" aria-label="桌面编程会话配置">
    <HelpHeading as="h3" topic="DesktopWorkerFields">绑定 Qoder 会话</HelpHeading>
    <label htmlFor={`${idPrefix}-session-id`}>绑定的 Qoder 会话 ID</label>
    <input ref={sessionField} id={`${idPrefix}-session-id`} value={sessionId} disabled={disabled} required maxLength={80}
      placeholder="粘贴目标聊天的完整会话 ID（UUID）" autoComplete="off" spellCheck={false}
      onChange={event => {
        const next = event.target.value;
        setSessionId(next); setError(""); setNotice("");
        if (worker && normalize(next) !== normalize(worker.binding.observation.sessionId ?? "")) onChange(undefined);
      }}/>
    <div className="connection-test" data-connection-actions>
      <button type="button" className="primary" disabled={disabled || !valid || !project || !send} onClick={() => void bind()}>
        {phase !== "idle" ? "正在核对并绑定…" : "测试并绑定会话"}
      </button>
      <div className="connection-test-feedback">
        <p className="secondary">在 Qoder 中打开对应聊天并保持空闲，选择模型后填写会话 ID。绑定时自动核对窗口、会话与模型。</p>
        {!!sessionId && !valid && <p>请填写完整的 UUID 格式会话 ID。</p>}
        {notice && <p role="status">{notice}</p>}
        {error && <p role="alert"><ErrorText error={error}/></p>}
      </div>
    </div>
    {worker && <>
      <p>已绑定模型 · {worker.modelLabel}</p>
      {qoderUpgradeBlock(worker.binding, data.desktopAccess?.trials ?? [], data.desktopAccess?.bridgeSha256 ?? "") &&
        <p role="status">{qoderUpgradeBlock(worker.binding, data.desktopAccess?.trials ?? [], data.desktopAccess?.bridgeSha256 ?? "")}</p>}
      <p className="secondary desktop-bound-directory">执行目录 · {worker.projectRoot}</p>
      {isolationEnabled && <p className="secondary">执行时使用本项目的独立 Windows 账户及已分配目录。登录执行账户后，在该账户的 Qoder 中重新绑定会话。</p>}
      <label className="checkbox-label"><input type="checkbox" required disabled={disabled}
        checked={worker.workspaceConfirmed && worker.externalToolsAcknowledged}
        onChange={event => onChange({ ...worker, workspaceConfirmed: event.target.checked, externalToolsAcknowledged: event.target.checked })}/>
        我已核对 Qoder 的工作目录与模型，并确认使用该应用的现有工具权限</label>
      <details><summary>执行范围与绑定详情</summary><DesktopExecutionReview worker={worker}/></details>
    </>}
  </section>;
}

export function DesktopExecutionReview({worker}:{worker:DesktopWorkerSelection}) {
  const b=worker.binding,o=b.observation;
  return <section aria-label="桌面执行授权范围">
    <dl><dt>桌面应用</dt><dd>{o.window.app==='qoder'?'Qoder':'WorkBuddy'} {o.window.version}</dd>
      <dt>已关联聊天</dt><dd>{b.label} · {o.window.title}</dd>
      <dt>模型显示名（UI观察）</dt><dd>{worker.modelLabel}</dd>
      <dt>项目目录</dt><dd>{worker.projectRoot}</dd>
      <dt>会话身份证据</dt><dd>{o.sessionId?`Qoder界面路由UUID：${o.sessionId}`:'WorkBuddy内容指纹与用户关联；没有供应商会话ID'}</dd>
      <dt>工具权限</dt><dd>沿用外部应用配置。项目目录由用户核对，当前未由工作台验证外部应用实际工作目录；该关联不提供文件系统沙箱。</dd>
      <dt>费用</dt><dd>桌面订阅或调用费用不可精确计量；计划仅限定请求次数与等待时长，不能保证外部任务在超时后停止。</dd>
    </dl>
    <p className="secondary">执行前仍须核对同一会话、模型、草稿和忙闲状态。更改此关联、模型或目录后，需要重新规划并批准。</p>
    <details><summary>原会话观察依据</summary><p>{new Date(o.capturedAt).toLocaleString('zh-CN')} · {o.window.exe}</p><p>关联摘要：{b.id}</p><p>桥接版本摘要：{b.bridgeSha256}</p></details>
  </section>;
}
