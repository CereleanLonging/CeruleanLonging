import { useAccessPreference } from "./accessPreferences";
import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { ApiModelPicker } from "./ApiModelPicker";
import type { ApiModelAssignment } from "./apiModels";
import { useEffect, useRef, useState } from "react";
import type { Command, CredentialProvider, ProviderConnection, ProviderEndpoint, Snapshot } from "./contracts";

export const providers = [
  {id:"deepseek",name:"DeepSeek",title:"DeepSeek 执行模型",target:{kind:"deepSeek"}},
  {id:"anthropic",name:"Claude",title:"Claude API",target:{kind:"anthropic"}},
  {id:"qwen",name:"Qwen",title:"Qwen / 百炼 API",target:{kind:"qwenBeijing",workspaceId:""}},
  {id:"kimi",name:"Kimi",title:"Kimi API（中国站）",target:{kind:"kimiChina"}},
  {id:"a6api",name:"A6API",title:"A6API 中转",target:{kind:"a6Api"}},
  {id:"grok",name:"Grok",title:"xAI Grok API",target:{kind:"grok"}},
] satisfies {id:CredentialProvider;name:string;title:string;target:ProviderEndpoint}[];
type Entry=typeof providers[number];

export function ProviderCard({entry,status,send,busy,connected,view="all",idPrefix=entry.id,scope,fixedTarget,modelAssignments}:{modelAssignments?:ApiModelAssignment[];fixedTarget?:ProviderEndpoint;scope?:string;entry:Entry;status:ProviderConnection|undefined;send:(command:Command,onError?:(message:string)=>void)=>Promise<boolean>;busy:boolean;connected:boolean;view?:"all"|"api"|"key";idPrefix?:string}) {
  const initial=status?.target??entry.target;
  const allowed=entry.id==="qwen"?["qwenBeijing","qwenSingapore","qwenHongKong"]:entry.id==="a6api"?["a6Api","a6ApiVip"]:[entry.target.kind];
  const [region,setRegion,regionError]=useAccessPreference(scope,`endpoint:${entry.id}`,initial.kind,allowed);
  const [workspace,setWorkspace,workspaceError]=useAccessPreference(scope,`workspace:${entry.id}`,initial.kind==="qwenBeijing"?initial.workspaceId:"");
  const target:ProviderEndpoint=region==="qwenBeijing"?{kind:"qwenBeijing",workspaceId:workspace}:{kind:region as Exclude<ProviderEndpoint["kind"],"qwenBeijing">};
  const [key,setKey]=useState(""),[pending,setPending]=useState(false);
  const [action,setAction]=useState<"save"|"remove"|"check"|null>(null);
  const [actionError,setActionError]=useState("");
  const [modelRequest,setModelRequest]=useState(0);
  const lock=useRef(false);
  const unsavedKey=key.length>0;
  const saved=status?.keyConfigured===true,checking=status?.state==="checking",disabled=busy||pending||checking||!connected;
  const actual:ProviderEndpoint=fixedTarget??target;
  const valid=actual.kind!=="qwenBeijing"||/^[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$/.test(actual.workspaceId);
  async function act(command: Command) {
    if (disabled || lock.current || (command.type === "checkProviderModels" && unsavedKey)) return;
    lock.current = true;
    setPending(true);
    setActionError("");
    setAction(command.type === "setProviderKey" ? "save"
      : command.type === "removeProviderKey" ? "remove" : "check");
    try {
      let reportedError = false;
      const accepted = await send(command, message => { reportedError = true; setActionError(message); });
      if (!accepted) setActionError(message => message || "操作未完成，请核对当前状态与页面提示。");
      if (accepted && !reportedError && command.type !== "removeProviderKey") setModelRequest(value => value + 1);
    } catch (error) {
      setActionError(String(error));
    } finally {
      lock.current = false;
      setPending(false);
    }
  }
  const sameTarget=!!status?.target&&status.target.kind===actual.kind&&(actual.kind!=="qwenBeijing"||(status.target.kind==="qwenBeijing"&&status.target.workspaceId===actual.workspaceId));
  useEffect(()=>{
    if(!fixedTarget&&status?.state==="listed"&&sameTarget){setRegion(actual.kind);if(actual.kind==="qwenBeijing")setWorkspace(actual.workspaceId);}
  },[status?.state,status?.checkedAt,sameTarget]);
  const label=checking?"正在读取模型列表":unsavedKey?"新 Key 尚未保存":!saved?"尚未配置":!sameTarget&&["listed","failed"].includes(status?.state??"")?"接入选项已更改，需重新检查":status?.state==="listed"?"列表已读取；执行待验证":status?.state==="failed"?"检查未通过":"已加密保存，尚未验证";
  return <section className="panel" aria-labelledby={`${idPrefix}-credential-heading`}>
    <div className="section-heading"><HelpHeading as="h3" topic="ProviderConnections" id={`${idPrefix}-credential-heading`}>{entry.name} {view==="key"?"Key":"API"}</HelpHeading><span className="badge">{label}</span></div>
    <p className="secondary">{view==="key"?"凭证按供应商加密保存；保存 Key 不会发出模型请求。":"模型列表检查只读取目录，不发送编程任务；目录返回不代表模型生成、工具调用或额度已验证。"}</p>
    {(regionError||workspaceError)&&<p role="alert">端点选项自动保存失败，输入仍保留；请重新填写后重试。</p>}
    {!fixedTarget&&entry.id==="a6api"&&<>
      <label htmlFor={`${idPrefix}-origin`}>中转站地址（须与 Key 来源一致）</label>
      <select id={`${idPrefix}-origin`} disabled={disabled} value={target.kind} onChange={event=>{const kind=event.target.value;if(kind==="a6Api"||kind==="a6ApiVip")setRegion(kind);}}>
        <option value="a6Api">https://api.a6api.com/v1</option><option value="a6ApiVip">https://api.a6api.vip/v1</option>
      </select>
      <p className="secondary">只向所选地址读取模型列表，不自动切换域名。当前工作台保存一枚 A6API Key；更换地址前请核实凭证适用范围。</p>
    </>}
    {view!=="api"&&<form onSubmit={event=>{event.preventDefault();if(disabled||lock.current)return;const value=key;setKey("");void act({type:"setProviderKey",provider:entry.id,key:value});}}>
      <label htmlFor={`${idPrefix}-key`}>{entry.name} API Key</label>
      <input id={`${idPrefix}-key`} type="password" autoComplete="off" spellCheck={false} value={key} onChange={event=>setKey(event.target.value)} minLength={16} maxLength={512} required disabled={disabled} placeholder={saved?"已保存；输入新 Key 可替换":"在本机粘贴 Key，无需发送到聊天"}/>
      <p className="secondary">按当前 Windows 用户及工作台目录加密；提交后清空输入，其他供应商的密钥保持独立。</p>
      <div className="integration-actions credential-actions">
        <button type="submit" className="primary" disabled={disabled}>{pending&&action==="save"?"正在保存…":`加密保存 ${entry.name} Key`}</button>
        <button type="button" className="secondary-button" disabled={disabled||!saved} onClick={()=>void act({type:"removeProviderKey",provider:entry.id})}>{pending&&action==="remove"?"正在删除…":`删除本机 ${entry.name} Key`}</button>
      </div>
      {!pending&&!actionError&&action==="save"&&<p role="status">{saved?`${entry.name} Key 已加密保存，可以测试连通。`:"保存操作已接收，凭证状态尚未确认，请重新读取状态。"}</p>}
      {!pending&&!actionError&&action==="remove"&&<p role="status">{saved?"删除操作已接收，凭证状态尚未确认，请重新读取状态。":`${entry.name} Key 已从本机删除。`}</p>}
    </form>}
    {actionError&&<p role="alert"><ErrorText error={actionError}/></p>}
    {!fixedTarget&&entry.id==="qwen"&&<>
      <label htmlFor={`${idPrefix}-region`}>百炼地域（须与 Key 一致）</label>
      <select id={`${idPrefix}-region`} disabled={disabled} value={target.kind} onChange={event=>{const kind=event.target.value;if(kind==="qwenBeijing")setRegion(kind);else if(kind==="qwenSingapore"||kind==="qwenHongKong")setRegion(kind);}}>
        <option value="qwenBeijing">北京</option><option value="qwenSingapore">新加坡</option><option value="qwenHongKong">中国香港</option>
      </select>
      {target.kind==="qwenBeijing"&&<><label htmlFor={`${idPrefix}-workspace`}>北京业务空间 ID</label><input id={`${idPrefix}-workspace`} value={workspace} onChange={e=>setWorkspace(e.target.value)} maxLength={63} disabled={disabled} placeholder="从百炼控制台复制业务空间 ID"/></>}
      <p className="secondary">使用所选地域的按量 API Key；Coding Plan / Token Plan 另属专用接入。检查不会自动改用其他地域。</p>
    </>}
    <div className="connection-test">
      <button type="button" className="primary" disabled={disabled||!saved||!valid||unsavedKey} onClick={()=>void act({type:"checkProviderModels",target:actual})}>{checking?"测试中…":"测试连通"}</button>
      <div className="connection-test-feedback">
    <p className="secondary">验证 {entry.name} API 认证并读取所选端点的模型目录。</p>
    {!saved&&<p className="secondary">请先保存 {entry.name} Key，再测试连通。</p>}
    {unsavedKey&&<p role="status">输入的新 Key 尚未保存，请先点击“加密保存”。测试连通只使用已保存的 Key。</p>}
    {!valid&&<p className="secondary">请填写有效的北京业务空间 ID，再测试连通。</p>}
    {checking&&<p role="status">正在验证 {entry.name} API 认证和模型目录…</p>}
    {!unsavedKey&&!pending&&sameTarget&&status?.state==="listed"&&<p role="status">测试通过 · {entry.name} API 认证和模型目录读取成功。</p>}
    {!unsavedKey&&sameTarget&&status?.error&&<p role="alert">测试未通过 · <ErrorText error={status.error}/></p>}
      </div>
    </div>
    <>
    {sameTarget&&status?.endpoint&&<p className="secondary" style={{overflowWrap:"anywhere"}}>本次检查端点：{status.endpoint}</p>}
    {sameTarget&&status?.checkedAt&&<p className="secondary">检查时间：{new Date(status.checkedAt).toLocaleString("zh-CN",{hour12:false})}；重启后需重新检查。</p>}
    {modelAssignments && <ApiModelPicker target={{kind:"provider",endpoint:actual}} assignments={modelAssignments}
      models={sameTarget ? status?.models ?? [] : []} keyConfigured={saved} ready={sameTarget && status?.state==="listed"}
      checking={checking} disabled={disabled||unsavedKey} valid={valid} limited={status?.listLimited} failed={sameTarget && status?.state==="failed"}
      request={modelRequest} autoPrompt={view==="key"} onCheck={()=>void act({type:"checkProviderModels",target:actual})}/>}
    {sameTarget&&status?.state==="listed"&&<details><summary>已读取 {status.models.length} 个模型{status.listLimited?"（部分列表）":""}</summary>
      <p className="secondary">{status.listLimited?"达到当前页或展示上限，未返回的模型不能据此判定不可用。":"模型名称来自本次返回，执行能力仍需逐项验证。"}</p>
      {status.models.length?<ul>{status.models.map(id=><li key={id}>{id}</li>)}</ul>:<p>目录返回空列表，请核对账户及地域。</p>}
    </details>}
    </>
  </section>;
}
export function ProviderConnections({data,send,busy}:{data:Snapshot|null;send:(command:Command)=>Promise<boolean>;busy:boolean}){
  return <>{providers.map(entry=><ProviderCard key={entry.id} entry={entry} scope={data?.dataDirectory} status={data?.providerConnections?.find(s=>s.provider===entry.id)} send={send} busy={busy} connected={!!data}/>)}</>;
}
