import { useState } from "react";
import type { Command, Snapshot } from "./contracts";
import { CodexConnection } from "./CodexConnection";
import { DesktopSessions } from "./DesktopSessions";
import { OpenAiConnection } from "./OpenAiConnection";
import { ProviderCard, providers } from "./ProviderConnections";

export type IntegrationRole = "chief" | "deputy" | "worker";
type Props = { data: Snapshot; busy: boolean; send: (command: Command) => Promise<boolean>; configure: (role: IntegrationRole) => void };
type Method = "api" | "key" | "cli" | "desktop";
const roleNames = { chief: "主指挥", deputy: "副指挥", worker: "执行端" };

function ApiEntry({ role, view, data, busy, send }: Omit<Props,"configure"> & { role: IntegrationRole; view: "api" | "key" }) {
  const [provider, setProvider] = useState("openai");
  const prefix = `integration-${role}-${view}`;
  const entry = providers.find(p => p.id === provider);
  return <div className="integration-entry">
    <label htmlFor={`${prefix}-provider`}>{view === "key" ? "凭证供应商" : "API 服务"}</label>
    <select id={`${prefix}-provider`} value={provider} disabled={busy} onChange={e => setProvider(e.target.value)}>
      <option value="openai">OpenAI</option>
      {providers.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}
    </select>
    {view === "key" && <p className="secondary">同一工作台内，各角色使用按供应商保存的同一枚 Key。替换或删除会影响使用该供应商的项目；Key 本身不指定任务模型。</p>}
    {entry ? <ProviderCard key={`${prefix}-${provider}`} entry={entry} status={data.providerConnections?.find(c => c.provider === entry.id)} busy={busy} send={send} connected view={view} idPrefix={`${prefix}-${provider}`}/>
      : <OpenAiConnection key={prefix} status={data.openaiConnection} busy={busy} send={send} connected view={view} idPrefix={prefix}/>}
  </div>;
}

function WorkerCli({ data, busy, send, configure }: Props) {
  const [program, setProgram] = useState("codex");
  return <div className="integration-entry">
    <label htmlFor="integration-worker-cli-program">CLI 程序</label>
    <select id="integration-worker-cli-program" value={program} disabled={busy} onChange={e => setProgram(e.target.value)}>
      <option value="codex">Codex</option><option value="omp">OMP</option>
    </select>
    {program === "codex" ? <CodexConnection status={data.codexConnection} busy={busy} send={send} connected/>
      : <div className="integration-note"><h3>OMP</h3><p>在项目执行端配置中填写 OMP 程序路径、模型和运行目录。当前未提供独立的 OMP 目录扫描或登录检查。</p>
        <button type="button" className="secondary-button" disabled={busy || !data.workflows.length} onClick={() => configure("worker")}>配置 OMP 执行模型</button></div>}
  </div>;
}

function ConfigureButton({ role, data, busy, configure }: Pick<Props,"data"|"busy"|"configure"> & { role: IntegrationRole }) {
  return <button type="button" className="secondary-button" disabled={busy || !data.workflows.length} onClick={() => configure(role)}>配置{roleNames[role]}模型</button>;
}

export function IntegrationHub(props: Props) {
  const { data, busy, send, configure } = props;
  const [chiefMethod, setChiefMethod] = useState<Method>("api");
  return <div className="integration-hub">
    <p className="secondary">按角色管理接入。连接检查和凭证保存不会切换已批准任务的模型；选择任务模型时进入对应项目配置，保存后重新审阅方案。</p>
    {!data.workflows.length && <p className="secondary">请先在“项目设置”关联项目目录，再配置各角色的任务模型。接入检查与 Key 保存可先完成。</p>}
    <section className="panel integration-role" aria-labelledby="integration-chief-title">
      <div className="integration-role-heading"><div><span className="integration-role-eyebrow">规划与收口</span><h2 id="integration-chief-title">主指挥</h2></div><ConfigureButton role="chief" {...props}/></div>
      <label htmlFor="integration-chief-method">主指挥接入方式</label>
      <select id="integration-chief-method" value={chiefMethod} disabled={busy} onChange={e => setChiefMethod(e.target.value as Method)}>
        <option value="api">API 连接</option><option value="key">Key 管理</option><option value="desktop">桌面端 · 当前 Codex 聊天</option><option value="cli" disabled>CLI · 当前未开放主指挥接入</option>
      </select>
      <div className="integration-single-entry" key={chiefMethod}>
        {(chiefMethod === "api" || chiefMethod === "key") && <ApiEntry role="chief" view={chiefMethod} data={data} busy={busy} send={send}/>}
        {chiefMethod === "desktop" && <div className="integration-note"><h3>绑定当前 Codex 聊天</h3><p>此入口使用已打开的 Codex 聊天。由该聊天提供会话标识，再到主指挥项目配置中绑定；无需扫描本地程序目录。</p><ConfigureButton role="chief" {...props}/></div>}
      </div>
    </section>

    <section className="panel integration-role" aria-labelledby="integration-deputy-title">
      <div className="integration-role-heading"><div><span className="integration-role-eyebrow">协调与监督</span><h2 id="integration-deputy-title">副指挥</h2></div><ConfigureButton role="deputy" {...props}/></div>
      <p className="secondary">副指挥当前通过 API 接入。可按项目选择不同供应商与模型，监督范围仍由批准方案决定。</p>
      <details className="integration-group"><summary>API 连接</summary><ApiEntry role="deputy" view="api" data={data} busy={busy} send={send}/></details>
      <details className="integration-group"><summary>Key 管理</summary><ApiEntry role="deputy" view="key" data={data} busy={busy} send={send}/></details>
    </section>

    <section className="panel integration-role" aria-labelledby="integration-worker-title">
      <div className="integration-role-heading"><div><span className="integration-role-eyebrow">执行与交付</span><h2 id="integration-worker-title">执行端</h2></div><ConfigureButton role="worker" {...props}/></div>
      <p className="secondary">按任务选择默认执行模型或附加执行配置。已有模型工作目录继续复用，模型切换需要保存新版项目配置。</p>
      <details className="integration-group"><summary>API 连接</summary><ApiEntry role="worker" view="api" data={data} busy={busy} send={send}/></details>
      <details className="integration-group"><summary>Key 管理</summary><ApiEntry role="worker" view="key" data={data} busy={busy} send={send}/></details>
      <details className="integration-group"><summary>CLI 接入</summary><WorkerCli {...props}/></details>
      <details className="integration-group"><summary>桌面端接入</summary><DesktopSessions data={data} busy={busy} send={send}/></details>
    </section>

    <details className="panel integration-group integration-catalog"><summary>接入能力与支持范围</summary>
      <p className="secondary">各项能力以当前验证结果为准。任务执行需要先批准具体方案。</p>
      <div className="adapter-grid">{data.adapters.map(adapter => <article className="panel" key={adapter.id}>
        <div className="section-heading"><h3>{adapter.name}</h3><span className="badge">{adapter.status === "partial" ? "部分接入" : adapter.status === "prototype" ? "原型验证" : "待接入"}</span></div>
        <span className="transport">{adapter.transport}</span><p>{adapter.note}</p>
      </article>)}</div>
    </details>
  </div>;
}
