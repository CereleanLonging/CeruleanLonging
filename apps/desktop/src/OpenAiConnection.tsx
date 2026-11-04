import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { useState } from "react";
import type { Command, OpenAiConnection as Status } from "./contracts";
export function OpenAiConnection({status,send,busy,connected,view="all",idPrefix="openai"}:{status:Status|undefined;send:(command:Command)=>Promise<boolean>;busy:boolean;connected:boolean;view?:"all"|"api"|"key";idPrefix?:string}) {
  const [key,setKey]=useState("");
  const checking=status?.state==="checking";
  const labels:Record<string,string>={notConfigured:"尚未配置",unchecked:"已加密保存，待检查",checking:"正在检查官方 API",available:"官方 API 认证通过",failed:"连接检查未通过"};
  return <section className="panel">
    <div className="section-heading"><HelpHeading as="h3" topic="OpenAiConnection">OpenAI {view==="key"?"Key":"API"}</HelpHeading><span className="badge">{labels[status?.state??"notConfigured"]??"尚未配置"}</span></div>
    <p className="secondary">使用官方 API Key。API 用量与 Codex 订阅分开计费；角色能力由项目配置和已批准方案决定。</p>
    {view!=="api"&&<form onSubmit={async event=>{event.preventDefault();if(busy||checking||!connected)return;const value=key;setKey("");await send({type:"setOpenAiKey",key:value});}}>
      <label htmlFor={`${idPrefix}-key`}>OpenAI API Key</label>
      <input id={`${idPrefix}-key`} disabled={busy||checking||!connected} type="password" autoComplete="off" spellCheck={false} value={key} onChange={event=>setKey(event.target.value)} placeholder={status?.keyConfigured?"已保存；输入新 Key 可替换":"在本机粘贴 Key，无需发送到聊天"} minLength={16} maxLength={512} required />
      <p className="secondary">以当前 Windows 用户加密保存。不会在这里回显；更换用户或迁移工作台数据目录后需重新配置。</p>
      <button className="primary" disabled={busy||checking||!connected}>加密保存 Key</button>
    </form>}
    <div className="integration-actions">
      {view!=="key"&&<button type="button" className="primary" disabled={busy||checking||!connected||!status?.keyConfigured} onClick={()=>void send({type:"checkOpenAi"})}>检查官方 API 连接</button>}
      {view!=="api"&&<button type="button" className="secondary-button" disabled={busy||checking||!connected||!status?.keyConfigured} onClick={()=>void send({type:"removeOpenAiKey"})}>删除本机 Key</button>}
    </div>
    {view!=="key"&&<><p className="secondary">连接检查只读取可见模型，不生成任务。请先在 Key 分组保存凭证。</p>
      {status?.error && <p role="alert"><ErrorText error={status.error}/></p>}
      {!!status?.models.length && <details><summary>可见模型（{status.models.length}）</summary><p>{status.models.join("、")}</p></details>}
    </>}
  </section>;
}
