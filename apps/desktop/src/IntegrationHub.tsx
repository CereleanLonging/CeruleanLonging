import { useState } from "react";
import { HelpHeading } from "./HelpHeading";
import { useAccessPreference } from "./accessPreferences";
import type { Command, Snapshot } from "./contracts";
import { CodexConnection } from "./CodexConnection";
import { DesktopSessions } from "./DesktopSessions";
import { OpenAiConnection } from "./OpenAiConnection";
import { ProviderCard, providers } from "./ProviderConnections";

export type IntegrationRole = "chief" | "deputy" | "worker";
export type ChiefMethod = "api" | "desktop";
export type ConfigurationRequest = { token: number; role: IntegrationRole; chiefMethod?: ChiefMethod; projectId?: string };
type Props = { data: Snapshot; busy: boolean; send: (command: Command) => Promise<boolean>; configure: (role: IntegrationRole, chiefMethod?: ChiefMethod) => void };
const roleNames = { chief: "主指挥", deputy: "副指挥", worker: "执行端" };

function ApiEntry({ role, view, data, busy, send }: Omit<Props,"configure"> & { role: IntegrationRole; view: "api" | "key" }) {
  const [provider, setProvider, preferenceError] = useAccessPreference(data.dataDirectory, `provider:${role}:${view}`, "openai", ["openai",...providers.map(p=>p.id)]);
  const prefix = `integration-${role}-${view}`;
  const entry = providers.find(p => p.id === provider);
  return <div className="integration-entry">
    {preferenceError&&<p role="alert">接入选项自动保存失败，请重新选择后重试。</p>}
    <label htmlFor={`${prefix}-provider`}>{view === "key" ? "凭证供应商" : "API 服务"}</label>
    <select id={`${prefix}-provider`} value={provider} disabled={busy} onChange={e => setProvider(e.target.value)}>
      <option value="openai">OpenAI</option>
      {providers.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}
    </select>
    {view === "key" && <p className="secondary">同一工作台内，各角色使用按供应商保存的同一枚 Key。替换或删除会影响使用该供应商的项目；Key 本身不指定任务模型。</p>}
    {entry ? <ProviderCard key={`${prefix}-${provider}`} entry={entry} scope={data.dataDirectory} status={data.providerConnections?.find(c => c.provider === entry.id)} busy={busy} send={send} connected view={view} idPrefix={`${prefix}-${provider}`}/>
      : <OpenAiConnection key={prefix} status={data.openaiConnection} busy={busy} send={send} connected view={view} idPrefix={prefix}/>}
  </div>;
}

function WorkerCli({ data, busy, send, configure }: Props) {
  const [program, setProgram, preferenceError] = useAccessPreference(data.dataDirectory, "worker-cli", "codex", ["codex","omp"]);
  return <div className="integration-entry">
    {preferenceError&&<p role="alert">CLI 选项自动保存失败，请重新选择后重试。</p>}
    <label htmlFor="integration-worker-cli-program">CLI 程序</label>
    <select id="integration-worker-cli-program" value={program} disabled={busy} onChange={e => setProgram(e.target.value)}>
      <option value="codex">Codex</option><option value="omp">OMP</option>
    </select>
    {program === "codex" ? <CodexConnection scope={data.dataDirectory} status={data.codexConnection} busy={busy} send={send} connected/>
      : <div className="integration-note"><HelpHeading as="h3" topic="IntegrationHub">OMP</HelpHeading><p>在项目执行端配置中填写 OMP 程序路径、模型和运行目录。当前未提供独立的 OMP 目录扫描或登录检查。</p>
        <button type="button" className="secondary-button" disabled={busy} onClick={() => configure("worker")}>配置 OMP 执行模型</button></div>}
  </div>;
}

function ConfigureButton({ role, chiefMethod, busy, configure }: Pick<Props,"busy"|"configure"> & { role: IntegrationRole; chiefMethod?: ChiefMethod }) {
  return <button type="button" className="secondary-button" disabled={busy} onClick={() => configure(role, chiefMethod)}>配置{roleNames[role]}模型</button>;
}

export function IntegrationHub(props: Props) {
  const { data, busy, send, configure } = props;
  const [chiefMethod, setChiefMethod] = useState<"api"|"key"|"desktop"|null>(null);
  return <div className="integration-hub">
    <p className="secondary">先展开连接方式，再配置模型服务、凭证或已有登录。主指挥和副指挥各使用一个模型，执行端可按项目添加多个配置。</p>
    {!data.workflows.length && <p className="secondary">尚未关联项目目录，可先完成连接检查与 Key 保存，再通过本页的项目入口关联目录并选择角色模型。</p>}
    <section className="panel integration-role" aria-labelledby="integration-chief-title">
      <div className="integration-role-heading"><div><span className="integration-role-eyebrow">规划与收口</span><HelpHeading as="h2" topic="IntegrationHub" id="integration-chief-title">主指挥</HelpHeading></div><ConfigureButton role="chief" {...props}/></div>
      {(["api","key","desktop"] as const).map(method=><details className="integration-group" name="unbound-chief-method" key={method}
        onToggle={event=>{const open=event.currentTarget.open;setChiefMethod(current=>open?method:current===method?null:current);}}>
        <summary>{method==="api"?"API 连接":method==="key"?"Key 管理":"桌面端接入"}</summary>
        {chiefMethod===method&&(method==="desktop"?<div className="integration-note">
          <HelpHeading as="h3" topic="IntegrationHub">绑定当前 Codex 聊天</HelpHeading>
          <p>关联项目后，在主指挥的桌面端接入中填写 Codex 聊天标识。</p>
          <button type="button" className="secondary-button" disabled={busy} onClick={()=>configure("chief","desktop")}>绑定 Codex 聊天</button>
        </div>:<ApiEntry role="chief" view={method} data={data} busy={busy} send={send}/>)}
      </details>)}
    </section>

    <section className="panel integration-role" aria-labelledby="integration-deputy-title">
      <div className="integration-role-heading"><div><span className="integration-role-eyebrow">协调与监督</span><HelpHeading as="h2" topic="IntegrationHub" id="integration-deputy-title">副指挥</HelpHeading></div><ConfigureButton role="deputy" {...props}/></div>
      <p className="secondary">副指挥当前通过 API 接入。可按项目选择不同供应商与模型，监督范围仍由批准方案决定。</p>
      <details className="integration-group" name="unbound-deputy-method"><summary>API 连接</summary><ApiEntry role="deputy" view="api" data={data} busy={busy} send={send}/></details>
      <details className="integration-group" name="unbound-deputy-method"><summary>Key 管理</summary><ApiEntry role="deputy" view="key" data={data} busy={busy} send={send}/></details>
    </section>

    <section className="panel integration-role" aria-labelledby="integration-worker-title">
      <div className="integration-role-heading"><div><span className="integration-role-eyebrow">执行与交付</span><HelpHeading as="h2" topic="IntegrationHub" id="integration-worker-title">执行端</HelpHeading></div><ConfigureButton role="worker" {...props}/></div>
      <p className="secondary">按任务选择默认执行模型或附加执行配置。已有模型工作目录继续复用，模型切换需要保存新版项目配置。</p>
      <details className="integration-group"><summary>API 连接</summary><ApiEntry role="worker" view="api" data={data} busy={busy} send={send}/></details>
      <details className="integration-group"><summary>Key 管理</summary><ApiEntry role="worker" view="key" data={data} busy={busy} send={send}/></details>
      <details className="integration-group"><summary>CLI 接入</summary><WorkerCli {...props}/></details>
      <details className="integration-group"><summary>桌面端接入</summary><DesktopSessions data={data} busy={busy} send={send}/></details>
    </section>

    <details className="panel integration-group integration-catalog"><summary>接入能力与支持范围</summary>
      <p className="secondary">各项能力以当前验证结果为准。任务执行需要先批准具体方案。</p>
      <div className="adapter-grid">{data.adapters.map(adapter => <article className="panel" key={adapter.id}>
        <div className="section-heading"><HelpHeading as="h3" topic="IntegrationHub">{adapter.name}</HelpHeading><span className="badge">{adapter.status === "partial" ? "部分接入" : adapter.status === "prototype" ? "原型验证" : "待接入"}</span></div>
        <span className="transport">{adapter.transport}</span><p>{adapter.note}</p>
      </article>)}</div>
    </details>
  </div>;
}
