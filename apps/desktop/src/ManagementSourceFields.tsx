import type { ProjectSetupDraft, ProviderEndpoint } from "./contracts";
import { managementEndpoints } from "./ManagementProviders";

function selected(value: string): {kind: "provider"; endpoint: ProviderEndpoint} {
  const kind = value.slice("provider:".length) as ProviderEndpoint["kind"];
  return {kind:"provider",endpoint:kind === "qwenBeijing" ? {kind,workspaceId:""} : {kind}};
}

export function ManagementSourceFields({ draft, onChange }: {
  draft: ProjectSetupDraft;
  onChange: (draft: ProjectSetupDraft) => void;
}) {
  const sources = draft.managementSources ?? { chief: { kind: "openAi" }, deputy: { kind: "openAi" } };
  const external = sources.chief.kind === "codexSession";
  function workspace(role:"chief"|"deputy") {
    const source=sources[role];
    if(source.kind!=="provider" || source.endpoint.kind!=="qwenBeijing")return null;
    return <div><label htmlFor={`planning-${role}-workspace`}>{role==="chief"?"主指挥":"副指挥"} Qwen 北京 Workspace ID</label>
      <input id={`planning-${role}-workspace`} required maxLength={63} value={source.endpoint.workspaceId} onChange={e=>onChange({...draft,managementSources:{...sources,[role]:{kind:"provider",endpoint:{kind:"qwenBeijing",workspaceId:e.target.value}}}})}/></div>;
  }
  return <fieldset className="management-source-fields">
    <legend>主、副指挥来源</legend>
    <section className="role-model-fields" aria-label="主指挥模型配置"><h3>主指挥</h3>
    <label htmlFor="planning-chief-source">主指挥接入</label>
    <select id="planning-chief-source" value={sources.chief.kind === "provider" ? `provider:${sources.chief.endpoint.kind}` : sources.chief.kind} onChange={e => {
      const isSession = e.target.value === "codexSession";
      onChange({ ...draft, chiefModel: isSession ? "external-codex-session" : "",
        managementSources: { ...sources, chief: isSession ? { kind: "codexSession", sessionId: "" } : e.target.value === "openAi" ? { kind: "openAi" } : selected(e.target.value) } });
    }}>
      <option value="openAi">OpenAI API</option>
      <option value="codexSession">当前 Codex 聊天</option>
      {managementEndpoints.map(e=><option key={e.kind} value={`provider:${e.kind}`}>{e.label}</option>)}
    </select>
    {sources.chief.kind === "codexSession" && <>
      <label htmlFor="planning-chief-session">绑定的 Codex 会话标识</label>
      <input id="planning-chief-session" required maxLength={80} pattern="[A-Za-z0-9_-]+" value={sources.chief.sessionId}
        onChange={e => onChange({ ...draft, managementSources: { ...sources, chief: { kind: "codexSession", sessionId: e.target.value } } })} />
      <p className="secondary">由准备接入的 Codex 聊天提供标识。该聊天领取并回填方案；保存配置不会新建聊天或批准任务。</p>
    </>}
    <label htmlFor="planning-chief">主指挥模型 ID</label><input id="planning-chief" required readOnly={external} list="planning-chief-models" value={draft.chiefModel} onChange={e=>onChange({...draft,chiefModel:e.target.value})}/>
    {workspace("chief")}
    </section>
    <section className="role-model-fields" aria-label="副指挥模型配置"><h3>副指挥</h3>
    <label htmlFor="planning-deputy-source">副指挥接入</label>
    <select id="planning-deputy-source" value={sources.deputy.kind === "provider" ? `provider:${sources.deputy.endpoint.kind}` : sources.deputy.kind} onChange={e => onChange({ ...draft,
      deputyModel: e.target.value === "deepSeek" ? "deepseek-flash" : "",
      managementSources: { ...sources, deputy: e.target.value === "deepSeek" ? { kind: "deepSeek" } : e.target.value === "openAi" ? { kind: "openAi" } : selected(e.target.value) },
    })}>
      <option value="openAi">OpenAI API</option>
      <option value="deepSeek">DeepSeek API</option>
      {managementEndpoints.filter(e=>e.kind!=="deepSeek").map(e=><option key={e.kind} value={`provider:${e.kind}`}>{e.label}</option>)}
    </select>
    <label htmlFor="planning-deputy">副指挥模型 ID</label><input id="planning-deputy" required list="planning-deputy-models" value={draft.deputyModel} onChange={e=>onChange({...draft,deputyModel:e.target.value})}/>
    {workspace("deputy")}
    </section>
    <p className="secondary">主指挥负责规划与收口，副指挥负责分配与监督，编程模型另行选择。方案仍需用户批准；各 API 来源使用对应供应商的本机凭证。Qwen API 地域选项对应普通 API，Token Plan 沿用已配置的 Qoder 路线。</p>
    {external && <p role="status">执行完成并通过独立验收后，项目会等待绑定的 Codex 聊天回填收口决定。</p>}
    {sources.deputy.kind === "deepSeek" && <p className="secondary">DeepSeek 副指挥负责分配与监督建议，使用本机独立配置的 DeepSeek 凭证。</p>}
  </fieldset>;
}
