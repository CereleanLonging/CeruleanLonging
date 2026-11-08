import { CodexConnection } from "./CodexConnection";
import { useState } from "react";
import { HelpHeading } from "./HelpHeading";
import { useAccessPreference } from "./accessPreferences";
import { parseApiModelDefault, type ApiModelAssignment } from "./apiModels";
import type { Command, Snapshot } from "./contracts";
import { DesktopSessions } from "./DesktopSessions";
import { OpenAiConnection } from "./OpenAiConnection";
import { ProviderCard, providers } from "./ProviderConnections";
import { LocalConnectionTest } from "./LocalConnectionTest";
import { ModelOptionsButton } from "./ModelOptionsButton";
import type { ReasoningEffort } from "./contracts";
import { OmpConnectionTest } from "./OmpCredentials";

export type IntegrationRole = "chief" | "deputy" | "worker";
export type ChiefMethod = "api" | "desktop";
export type ConfigurationRequest = { token: number; role: IntegrationRole; chiefMethod?: ChiefMethod; projectId?: string };
type Props = { data: Snapshot; busy: boolean; send: (command: Command) => Promise<boolean>; configure: (role: IntegrationRole, chiefMethod?: ChiefMethod) => void };
const roleNames = { chief: "主指挥", deputy: "副指挥", worker: "执行端" };

function ApiEntry({ role, view, data, busy, send, modelPreference }: Omit<Props,"configure"> & { role: IntegrationRole; view: "api" | "key"; modelPreference: ReturnType<typeof useAccessPreference> }) {
  const [provider, setProvider, preferenceError] = useAccessPreference(data.dataDirectory, `provider:${role}:${view}`, "openai", ["openai",...providers.map(p=>p.id)]);
  const prefix = `integration-${role}-${view}`;
  const entry = providers.find(p => p.id === provider);
  const [modelDefault,setModelDefault,modelError] = modelPreference;
  const selectedModel = parseApiModelDefault(modelDefault);
  const modelAssignments: ApiModelAssignment[] = [{id:role,label:roleNames[role],model:selectedModel?.model??"",
    reasoningEffort:selectedModel?.reasoningEffort,target:selectedModel?.target??{kind:"openAi"},onSelect:(model,target,reasoningEffort)=>setModelDefault(JSON.stringify({target,model,reasoningEffort}))}];
  return <div className="integration-entry">
    {(preferenceError||modelError)&&<p role="alert">接入选项自动保存失败，请重新选择后重试。</p>}
    <label htmlFor={`${prefix}-provider`}>{view === "key" ? "凭证供应商" : "API 服务"}</label>
    <select id={`${prefix}-provider`} value={provider} disabled={busy} onChange={e => setProvider(e.target.value)}>
      <option value="openai">OpenAI</option>
      {providers.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}
    </select>
    {view === "key" && <p className="secondary">同一工作台内，各角色使用按供应商保存的同一枚 Key。替换或删除会影响使用该供应商的项目；Key 本身不指定任务模型。</p>}
    {entry ? <ProviderCard key={`${prefix}-${provider}`} entry={entry} scope={data.dataDirectory} status={data.providerConnections?.find(c => c.provider === entry.id)} busy={busy} send={send} connected view={view} idPrefix={`${prefix}-${provider}`} modelAssignments={modelAssignments}/>
      : <OpenAiConnection key={prefix} status={data.openaiConnection} busy={busy} send={send} connected view={view} idPrefix={prefix} modelAssignments={modelAssignments}/>}
    <p className="secondary">已选模型作为新关联项目的默认接入，可在项目中继续修改。</p>
  </div>;
}

function WorkerOmp({ data, busy, send, configure, role = "worker" }: Props & {role?: "deputy" | "worker"}) {
  const preferencePrefix = role === "deputy" ? "deputy-" : "";
  const [ompPath,setOmpPath] = useAccessPreference(data.dataDirectory,`${preferencePrefix}omp-test-executable`,"");
  const [ompModel,setOmpModel] = useAccessPreference(data.dataDirectory,`${preferencePrefix}omp-test-model`,"deepseek-flash");
  const [ompEffort,setOmpEffort] = useAccessPreference(data.dataDirectory,`${preferencePrefix}omp-test-effort`,"max",["","none","low","high","max"]);
  const [ompRuntime,setOmpRuntime] = useAccessPreference(data.dataDirectory,`${preferencePrefix}omp-test-runtime`,"");
  return <div className="integration-entry">
    <div className="integration-note"><HelpHeading as="h3" topic="IntegrationHub">OMP · 独立后台</HelpHeading>
        <label htmlFor={`integration-${role}-omp-path`}>OMP 程序完整路径</label><input id={`integration-${role}-omp-path`} value={ompPath} disabled={busy} onChange={e=>setOmpPath(e.target.value)}/>
        <label htmlFor={`integration-${role}-omp-model`}>OMP 模型 ID</label><input id={`integration-${role}-omp-model`} value={ompModel} disabled={busy} onChange={e=>{setOmpModel(e.target.value);setOmpEffort("max");}}/>
        <label htmlFor={`integration-${role}-omp-runtime`}>OMP 测试运行目录（可选）</label><input id={`integration-${role}-omp-runtime`} value={ompRuntime} disabled={busy} onChange={e=>setOmpRuntime(e.target.value)} placeholder="已存在的专用目录，留空使用工作台数据目录"/>
        <ModelOptionsButton allowDefault={false} model={ompModel} target={{kind:"provider",endpoint:{kind:"deepSeek"}}} effort={ompEffort as ReasoningEffort||undefined} disabled={busy} onChange={effort=>setOmpEffort(effort??"")}/>
        <OmpConnectionTest effort={ompEffort as ReasoningEffort||undefined} data={data} busy={busy} send={send} executable={ompPath} model={ompModel} runtimeParent={ompRuntime.trim()||undefined}/>
        <button type="button" className="secondary-button" disabled={busy} onClick={() => configure(role,"desktop")}>配置 OMP {roleNames[role]}模型</button></div>
  </div>;
}

function ConfigureButton({ role, chiefMethod, busy, configure }: Pick<Props,"busy"|"configure"> & { role: IntegrationRole; chiefMethod?: ChiefMethod }) {
  return <button type="button" className="secondary-button" disabled={busy} onClick={() => configure(role, chiefMethod)}>配置{roleNames[role]}模型</button>;
}

export function IntegrationHub(props: Props) {
  const { data, busy, send, configure } = props;
  const chiefModel = useAccessPreference(data.dataDirectory,"model-default:chief","");
  const deputyModel = useAccessPreference(data.dataDirectory,"model-default:deputy","");
  const workerModel = useAccessPreference(data.dataDirectory,"model-default:worker","");
  const [chiefMethod, setChiefMethod] = useState<"api"|"key"|"desktop"|null>(null);
  const [sessionId,setSessionId] = useState("");
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
          <label htmlFor="integration-chief-test-session">待测试的 Codex 会话标识</label>
          <input id="integration-chief-test-session" value={sessionId} maxLength={80} disabled={busy} onChange={e=>setSessionId(e.target.value)}/>
          <LocalConnectionTest scope={data.dataDirectory} busy={busy}
            request={sessionId.trim()?{kind:"codexSession",sessionId:sessionId.trim(),executable:data.codexConnection?.executable}:null}
            hint="填写已有会话标识，测试本机 Codex 登录和目标聊天读取权限。"/>
          <button type="button" className="secondary-button" disabled={busy} onClick={()=>configure("chief","desktop")}>绑定 Codex 聊天</button>
          <details><summary>程序升级检查</summary>
            <CodexConnection status={data.codexConnection} send={send} busy={busy} connected
              scope={data.dataDirectory} idPrefix="chief-upgrade"/>
          </details>
        </div>:<ApiEntry role="chief" view={method} data={data} busy={busy} send={send} modelPreference={chiefModel}/>)}
      </details>)}
    </section>

    <section className="panel integration-role" aria-labelledby="integration-deputy-title">
      <div className="integration-role-heading"><div><span className="integration-role-eyebrow">协调与监督</span><HelpHeading as="h2" topic="IntegrationHub" id="integration-deputy-title">副指挥</HelpHeading></div><ConfigureButton role="deputy" {...props}/></div>
      <p className="secondary">副指挥可选择 API 直连或桌面端下的 OMP 独立后台，每个项目使用一种接入。</p>
      <details className="integration-group" name="unbound-deputy-method"><summary>API 连接</summary><ApiEntry role="deputy" view="api" data={data} busy={busy} send={send} modelPreference={deputyModel}/></details>
      <details className="integration-group" name="unbound-deputy-method"><summary>桌面端接入</summary><WorkerOmp {...props} role="deputy"/></details>
      <details className="integration-group" name="unbound-deputy-method"><summary>Key 管理</summary><ApiEntry role="deputy" view="key" data={data} busy={busy} send={send} modelPreference={deputyModel}/></details>
    </section>

    <section className="panel integration-role" aria-labelledby="integration-worker-title">
      <div className="integration-role-heading"><div><span className="integration-role-eyebrow">执行与交付</span><HelpHeading as="h2" topic="IntegrationHub" id="integration-worker-title">执行端</HelpHeading></div><ConfigureButton role="worker" {...props}/></div>
      <p className="secondary">按任务选择默认执行模型或附加执行配置。Codex CLI 执行使用分配的隔离账户。已有模型工作目录继续复用，模型切换需要保存新版项目配置。</p>
      <details className="integration-group"><summary>CLI 接入</summary><div className="integration-entry"><p>在关联项目中选择执行程序和 CLI 模型，再确认思考强度。</p><ConfigureButton role="worker" {...props}/></div></details>
      <details className="integration-group"><summary>Key 管理</summary><ApiEntry role="worker" view="key" data={data} busy={busy} send={send} modelPreference={workerModel}/></details>
      <details className="integration-group"><summary>桌面端接入</summary>
        <WorkerOmp {...props}/>
        <div className="integration-entry">
          <p>关联项目后，填写 Qoder 会话 ID，点击“测试并绑定会话”即可读取模型并保存关联。</p>
          <ConfigureButton role="worker" {...props}/>
          <details><summary>高级会话工具与历史关联</summary><DesktopSessions data={data} busy={busy} send={send}/></details>
        </div>
      </details>
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
