import type { ReactNode } from "react";
import type { ApiTarget, Command, ProjectSetupDraft, Snapshot } from "./contracts";
import { apiTargetKey, managementApiTarget, type ApiModelAssignment } from "./apiModels";
import { HelpHeading } from "./HelpHeading";
import { endpointProvider } from "./ManagementProviders";
import { OpenAiConnection } from "./OpenAiConnection";
import { LocalConnectionTest } from "./LocalConnectionTest";
import { ProviderCard, providers } from "./ProviderConnections";
import { workerMethod, workerMethodLabels, type WorkerMethod } from "./ConnectionMethods";

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

function ManagerConnections({ role, projectId, desktop, fields, target, sessionId, onMethod, modelAssignments, ...connection }: ConnectionsProps & {
  role: "chief" | "deputy";
  projectId: string;
  desktop: boolean;
  fields: ReactNode;
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
    {role === "chief" && <details className="integration-group connection-method" name={name} data-method="desktop"
      onToggle={event => { if (event.currentTarget.open && !connection.busy) onMethod?.("desktop"); }}>
      <summary aria-disabled={connection.busy} onClick={event => { if (connection.busy) event.preventDefault(); }}>桌面端接入</summary>
      {desktop && <div className="connection-method-content">{fields}
        <LocalConnectionTest scope={connection.data.dataDirectory} busy={connection.busy}
          request={sessionId?.trim()?{kind:"codexSession",sessionId:sessionId.trim(),executable:connection.data.codexConnection?.executable}:null}
          hint="填写会话标识后测试，验证本机 Codex 登录及目标聊天的读取权限。"/>
      </div>}
    </details>}
    <details className="integration-group connection-method" name={name} data-method="key">
      <summary>Key 管理</summary>
      {target ? <ProjectApiConnections {...connection} target={target} prefix={`integration-${role}`} view="key" modelAssignments={modelAssignments}/>
        : <p className="secondary">当前桌面聊天使用应用自身的登录。选择 API 服务后，可在这里配置对应凭证。</p>}
    </details>
  </>;
}

export function ProjectConnections({ data, busy, send, draft, chief, deputy, worker, profiles, onChiefMethod, onDefaultWorker, onChange }: ConnectionsProps & {
  draft: ProjectSetupDraft;
  chief: ReactNode;
  deputy: ReactNode;
  worker: ReactNode;
  profiles: (method: WorkerMethod) => ReactNode;
  onChiefMethod: (method: "api" | "desktop") => void;
  onDefaultWorker: (method: WorkerMethod) => void;
  onChange: (draft: ProjectSetupDraft) => void;
}) {
  const connection = {data, busy, send};
  const defaultMethod = workerMethod(draft.workerAdapter);
  function managerAssignment(role: "chief" | "deputy"): ApiModelAssignment[] {
    const target = managementApiTarget(draft.managementSources?.[role]);
    if (!target) return [];
    return [{id:role, label:role === "chief" ? "主指挥" : "副指挥", target,
      model:role === "chief" ? draft.chiefModel : draft.deputyModel,
      onSelect:model => onChange({...draft, [role === "chief" ? "chiefModel" : "deputyModel"]:model})}];
  }
  const executions = [draft, ...(draft.profiles ?? [])];
  const workerAssignments: ApiModelAssignment[] = [];
  for (const [index, execution] of executions.entries()) {
    const target = execution.apiWorker?.target ?? (execution.workerAdapter === "omp" ? managementApiTarget({kind:"deepSeek"}) : undefined);
    if (!target) continue;
    workerAssignments.push({id:index === 0 ? "default" : `profile:${index-1}`, target, model:execution.workerModel,
      label:index === 0 ? "默认执行端" : `附加执行配置 ${draft.profiles?.[index-1].id}`,
      onSelect:model => {
        const patch = {workerModel:model, apiWorker:execution.apiWorker ? {...execution.apiWorker, model} : undefined};
        if (index === 0) onChange({...draft, ...patch});
        else onChange({...draft, profiles:draft.profiles?.map((profile, n) => n === index-1 ? {...profile, ...patch} : profile)});
      }});
  }
  const targets = [...new Map(workerAssignments.map(item => [apiTargetKey(item.target), item.target])).values()];
  return <div className="integration-hub project-connections">
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
        desktop={false} target={managementApiTarget(draft.managementSources?.deputy)} modelAssignments={managerAssignment("deputy")}/>
    </section>
    <section className="panel integration-role" aria-labelledby="integration-worker-title">
      <div className="integration-role-heading"><div><span className="integration-role-eyebrow">执行与交付 · 可多选</span>
        <HelpHeading as="h2" topic="IntegrationHub" id="integration-worker-title">执行端</HelpHeading>
      </div></div>
      <p className="secondary">可以展开多种连接方式，按需添加独立执行配置。默认执行端和附加配置会一起保存。</p>
      {(["api", "cli", "desktop"] as const).map(method => <details className="integration-group connection-method" data-method={method} key={method}>
        <summary>{workerMethodLabels[method]}</summary>
        <div className="connection-method-content">
          {defaultMethod === method ? <>
            <p className="connection-default-label">默认执行端</p>
            {worker}
            {draft.apiWorker && <ProjectApiConnections {...connection} target={draft.apiWorker.target} prefix="integration-worker" view="api" modelAssignments={workerAssignments.filter(item => item.id === "default")}/>}
          </> : <button type="button" className="secondary-button" disabled={busy} onClick={() => onDefaultWorker(method)}>
            将{workerMethodLabels[method]}设为默认执行端
          </button>}
          {profiles(method)}
        </div>
      </details>)}
      <details className="integration-group connection-method" data-method="key">
        <summary>Key 管理</summary>
        {targets.length ? targets.map((target, index) => <ProjectApiConnections {...connection} key={apiTargetKey(target)}
          target={target} prefix={`integration-worker-key-${index}`} view="key" modelAssignments={workerAssignments.filter(item => apiTargetKey(item.target) === apiTargetKey(target))}/>)
          : <p className="secondary">添加 API 或 OMP 执行配置后，可在这里管理对应凭证。</p>}
      </details>
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
