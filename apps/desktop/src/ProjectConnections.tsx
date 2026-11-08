import { useState, type ReactNode } from "react";
import type { ApiTarget, Command, ProjectSetupDraft, ReasoningEffort, Snapshot } from "./contracts";
import { apiTargetKey, managementApiTarget, type ApiModelAssignment } from "./apiModels";
import { HelpHeading } from "./HelpHeading";
import { endpointProvider } from "./ManagementProviders";
import { OpenAiConnection } from "./OpenAiConnection";
import { LocalConnectionTest } from "./LocalConnectionTest";
import { ProviderCard, providers } from "./ProviderConnections";
import { workerMethod, workerMethodLabels, type WorkerMethod } from "./ConnectionMethods";
import { OmpConnectionTest } from "./OmpCredentials";
import { ModelCredentialDialog } from "./ModelCredentialDialog";

type ConnectionsProps = {
  data: Snapshot;
  busy: boolean;
  send: (command: Command) => Promise<boolean>;
};

export function ProjectApiConnections({ target, prefix, data, busy, send, view, modelAssignments }: ConnectionsProps & {
  target: ApiTarget;
  prefix: string;
  view?: "api" | "key";
  modelAssignments?: ApiModelAssignment[];
}) {
  const provider = target.kind === "provider" ? endpointProvider(target.endpoint) : undefined;
  const entry = providers.find(item => item.id === provider);
  function content(selectedView: "api" | "key") {
    return <div className="integration-entry">
      {entry && target.kind === "provider"
        ? <ProviderCard key={`${entry.id}-${target.endpoint.kind}-${selectedView}`} entry={entry}
            fixedTarget={target.endpoint} scope={data.dataDirectory}
            status={data.providerConnections?.find(item => item.provider === provider)}
            busy={busy} send={send} connected view={selectedView} idPrefix={`${prefix}-${selectedView}-${entry.id}`} modelAssignments={modelAssignments}/>
        : <OpenAiConnection status={data.openaiConnection} busy={busy} send={send}
            connected view={selectedView} idPrefix={`${prefix}-${selectedView}-openai`} modelAssignments={modelAssignments}/>}
    </div>;
  }
  return <div className="project-connection-tools" data-connection-actions>
    {view ? content(view) : (["api", "key"] as const).map(selectedView => <details className="integration-group" key={selectedView}>
      <summary>{selectedView === "api" ? "API 连接" : "Key 管理"}</summary>
      {content(selectedView)}
    </details>)}
  </div>;
}

function ManagerConnections({ role, projectId, desktop, fields, target, sessionId, onMethod, modelAssignments, desktopTools, ...connection }: ConnectionsProps & {
  role: "chief" | "deputy";
  projectId: string;
  desktop: boolean;
  fields: ReactNode;
  desktopTools?: ReactNode;
  target?: ApiTarget;
  sessionId?: string;
  onMethod?: (method: "api" | "desktop") => void;
  modelAssignments: ApiModelAssignment[];
}) {
  const name = `connection-${projectId}-${role}`;
  return <>
    <p className="secondary">选择连接方式后展开配置，每个角色使用一个模型。</p>
    <details className="integration-group connection-method" name={name} data-method="api"
      onToggle={event => { if (event.currentTarget.open && !connection.busy) onMethod?.("api"); }}>
      <summary aria-disabled={connection.busy} onClick={event => { if (connection.busy) event.preventDefault(); }}>API 连接</summary>
      {!desktop && <div className="connection-method-content">{fields}
        {target && <ProjectApiConnections {...connection} target={target} prefix={`integration-${role}`} view="api" modelAssignments={modelAssignments}/>}
      </div>}
    </details>
    {<details className="integration-group connection-method" name={name} data-method="desktop"
      onToggle={event => { if (event.currentTarget.open && !connection.busy) onMethod?.("desktop"); }}>
      <summary aria-disabled={connection.busy} onClick={event => { if (connection.busy) event.preventDefault(); }}>桌面端接入</summary>
      {desktop && <div className="connection-method-content">{fields}
        {role === "chief" ? <LocalConnectionTest scope={connection.data.dataDirectory} busy={connection.busy}
          request={sessionId?.trim()?{kind:"codexSession",sessionId:sessionId.trim(),executable:connection.data.codexConnection?.executable}:null}
          hint="填写会话标识后核验 Codex 登录、协议和聊天。升级导致原程序位置失效时重新扫描，发现多个位置时需明确选择后再测试。"/> : desktopTools}
      </div>}
    </details>}
    <details className="integration-group connection-method" name={name} data-method="key">
      <summary>Key 管理</summary>
      {target ? <ProjectApiConnections {...connection} target={target} prefix={`integration-${role}`} view="key" modelAssignments={modelAssignments}/>
        : <p className="secondary">{role === "deputy" ? "OMP 使用工作台加密保存的 DeepSeek Key，可在桌面端接入中检查或补充凭证。" : "当前桌面聊天使用应用自身的登录。选择 API 服务后，可在这里配置对应凭证。"}</p>}
    </details>
  </>;
}

// One credential card per provider; endpoint checks and model assignments remain scoped.
function WorkerCredentialGroup({ assignments, ...connection }: ConnectionsProps & { assignments: ApiModelAssignment[] }) {
  const [selected, setSelected] = useState("");
  const targets = [...new Map(assignments.map(item => [apiTargetKey(item.target), item.target])).values()];
  const target = targets.find(item => apiTargetKey(item) === selected) ?? targets[0];
  if (!target) return null;
  const targetKey = apiTargetKey(target);
  const provider = target.kind === "provider" ? endpointProvider(target.endpoint) : "openai";
  const targetAssignments = assignments.filter(item => apiTargetKey(item.target) === targetKey);
  return <div className="worker-shared-credentials" data-connection-actions>
    {targets.length > 1 && <><label htmlFor={`worker-credential-target-${provider}`}>凭证检查目标</label>
      <select id={`worker-credential-target-${provider}`} value={targetKey} disabled={connection.busy} onChange={event => setSelected(event.target.value)}>
        {targets.map(item => <option key={apiTargetKey(item)} value={apiTargetKey(item)}>
          {assignments.filter(assignment => apiTargetKey(assignment.target) === apiTargetKey(item)).map(assignment => assignment.label).join("、")}
          {item.kind === "provider" ? ` · ${item.endpoint.kind}${item.endpoint.kind === "qwenBeijing" ? ` / ${item.endpoint.workspaceId}` : ""}` : " · OpenAI"}
        </option>)}
      </select></>}
    <ProjectApiConnections {...connection} target={target} prefix={`integration-worker-${provider}`} view="key" modelAssignments={targetAssignments}/>
  </div>;
}

export function ProjectConnections({ data, busy, send, draft, chief, deputy, worker, profiles, onChiefMethod, onDefaultWorker, onProfileModel, onChange }: ConnectionsProps & {
  draft: ProjectSetupDraft;
  chief: ReactNode;
  deputy: ReactNode;
  worker: ReactNode;
  profiles: (method: WorkerMethod) => ReactNode;
  onChiefMethod: (method: "api" | "desktop") => void;
  onDefaultWorker: (method: WorkerMethod) => void;
  onProfileModel: (index: number, model: string, effort?: ReasoningEffort) => void;
  onChange: (draft: ProjectSetupDraft) => void;
}) {
  const connection = {data, busy, send};
  const [deputyKeyPrompt, setDeputyKeyPrompt] = useState(false);
  const defaultMethod = workerMethod(draft.workerAdapter);
  function managerAssignment(role: "chief" | "deputy"): ApiModelAssignment[] {
    const target = managementApiTarget(draft.managementSources?.[role]);
    if (!target) return [];
    return [{id:role, label:role === "chief" ? "主指挥" : "副指挥", target,
      model:role === "chief" ? draft.chiefModel : draft.deputyModel,
      reasoningEffort:draft.managementSources?.[role === "chief" ? "chiefReasoningEffort" : "deputyReasoningEffort"],
      onSelect:(model, _target, reasoningEffort) => onChange({...draft, [role === "chief" ? "chiefModel" : "deputyModel"]:model, managementSources:{chief:{kind:"openAi"},deputy:{kind:"openAi"},...draft.managementSources,[role === "chief" ? "chiefReasoningEffort" : "deputyReasoningEffort"]:reasoningEffort}})}];
  }
  const executions = [draft, ...(draft.profiles ?? [])];
  const workerAssignments: ApiModelAssignment[] = [];
  for (const [index, execution] of executions.entries()) {
    const target = workerMethod(execution.workerAdapter) === "key" ? execution.apiWorker?.target : undefined;
    if (!target) continue;
    workerAssignments.push({id:index === 0 ? "default" : `profile:${index-1}`, target, model:execution.workerModel,
      label:index === 0 ? "默认执行端" : `附加执行配置 ${draft.profiles?.[index-1].id}`,
      reasoningEffort:execution.apiWorker?.reasoningEffort,
      onSelect:(model, _target, reasoningEffort) => {
        const patch = {workerModel:model, apiWorker:execution.apiWorker ? {...execution.apiWorker, model, reasoningEffort} : undefined};
        if (index === 0) onChange({...draft, ...patch});
        else onProfileModel(index-1, model, reasoningEffort);
      }});
  }
  const credentialGroups = new Map<string, ApiModelAssignment[]>();
  for (const assignment of workerAssignments) {
    const provider = assignment.target.kind === "provider" ? endpointProvider(assignment.target.endpoint) : "openai";
    credentialGroups.set(provider, [...(credentialGroups.get(provider) ?? []), assignment]);
  }
  return <div className="integration-hub project-connections">
    {deputyKeyPrompt && <ModelCredentialDialog {...connection} model={draft.deputyModel} focusTarget="planning-deputy"
      onClose={()=>setDeputyKeyPrompt(false)} onSelect={(model,target,reasoningEffort)=>{
        onChange({...draft,deputyModel:model,managementSources:{chief:{kind:"openAi"},...draft.managementSources,
          deputy:target,deputyReasoningEffort:reasoningEffort}});
        setDeputyKeyPrompt(false);
      }}/>}
    <section className="panel integration-role" aria-labelledby="integration-chief-title">
      <div className="integration-role-heading"><div><span className="integration-role-eyebrow">规划与收口 · 单选</span>
        <HelpHeading as="h2" topic="IntegrationHub" id="integration-chief-title">主指挥</HelpHeading>
      </div></div>
      <ManagerConnections {...connection} role="chief" projectId={draft.projectId} fields={chief}
        sessionId={draft.managementSources?.chief.kind === "codexSession" ? draft.managementSources.chief.sessionId : undefined}
        desktop={draft.managementSources?.chief.kind === "codexSession"} target={managementApiTarget(draft.managementSources?.chief)} onMethod={onChiefMethod} modelAssignments={managerAssignment("chief")}/>
    </section>
    <section className="panel integration-role" aria-labelledby="integration-deputy-title">
      <div className="integration-role-heading"><div><span className="integration-role-eyebrow">协调与监督 · 单选</span>
        <HelpHeading as="h2" topic="IntegrationHub" id="integration-deputy-title">副指挥</HelpHeading>
      </div></div>
      <ManagerConnections {...connection} role="deputy" projectId={draft.projectId} fields={deputy}
        desktop={draft.managementSources?.deputy.kind === "omp"}
        onMethod={method=>{
          const sources=draft.managementSources??{chief:{kind:"openAi" as const},deputy:{kind:"openAi" as const}};
          if ((method==="desktop") === (sources.deputy.kind==="omp")) return;
          onChange({...draft,deputyModel:method==="desktop"?"deepseek-flash":"",managementSources:{...sources,
            deputyReasoningEffort:method==="desktop"?"max":undefined,
            deputy:method==="desktop"?{kind:"omp",executable:""}:{kind:"openAi"}}});
        }}
        desktopTools={draft.managementSources?.deputy.kind === "omp" && <OmpConnectionTest {...connection}
          executable={draft.managementSources.deputy.executable} model={draft.deputyModel}
          onMissing={()=>setDeputyKeyPrompt(true)}
          effort={draft.managementSources.deputyReasoningEffort} runtimeParent={draft.managementSources.deputy.runtimeParent??undefined}/>}
        target={managementApiTarget(draft.managementSources?.deputy)} modelAssignments={managerAssignment("deputy")}/>
    </section>
    <section className="panel integration-role" aria-labelledby="integration-worker-title">
      <div className="integration-role-heading"><div><span className="integration-role-eyebrow">执行与交付 · 可多选</span>
        <HelpHeading as="h2" topic="IntegrationHub" id="integration-worker-title">执行端</HelpHeading>
      </div></div>
      <p className="secondary">API / Key 用于模型直连，CLI 用于独立账户下的 Codex 执行。桌面端可选择 Qoder 会话或 OMP 独立后台。默认执行端和附加配置会一起保存。</p>
      {(["key", "cli", "desktop"] as const).map(method => <details className="integration-group connection-method" data-method={method} key={method}>
        <summary>{workerMethodLabels[method]}</summary>
        <div className="connection-method-content">
          {method === "key" && credentialGroups.size > 0 && <section aria-label="执行端共享凭证">
            <h3>Key 接入</h3>
            <p className="secondary">同一供应商只保存一份 Key。在这里统一保存、测试连通和选模，模型分别应用到下方对应的执行配置。</p>
            {[...credentialGroups].map(([provider, assignments]) => <WorkerCredentialGroup key={provider} {...connection} assignments={assignments}/>)}
          </section>}
          {method === "key" && <h3>Key 执行管理设置</h3>}
          {defaultMethod === method ? <>
            <p className="connection-default-label">默认执行端</p>
            {worker}
          </> : <button type="button" className="secondary-button" disabled={busy} onClick={() => onDefaultWorker(method)}>
            将{workerMethodLabels[method]}设为默认执行端
          </button>}
          {profiles(method)}
        </div>
      </details>)}
    </section>
    <details className="panel integration-group integration-catalog">
      <summary>接入能力与支持范围</summary>
      <div className="adapter-grid">{data.adapters.map(adapter => <article className="panel" key={adapter.id}>
        <div className="section-heading"><HelpHeading as="h3" topic="IntegrationHub">{adapter.name}</HelpHeading>
          <span className="badge">{adapter.status === "partial" ? "部分接入" : adapter.status === "prototype" ? "原型验证" : "待接入"}</span>
        </div><span className="transport">{adapter.transport}</span><p>{adapter.note}</p>
      </article>)}</div>
    </details>
  </div>;
}
