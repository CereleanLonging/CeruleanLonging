import { ErrorText } from "./ErrorText";
import { useRef, useState } from "react";
import type { Command, CredentialProvider, ProviderConnection, ProviderEndpoint, Snapshot } from "./contracts";

const providers = [
  {id:"deepseek",name:"DeepSeek",title:"DeepSeek 执行模型",target:{kind:"deepSeek"}},
  {id:"anthropic",name:"Claude",title:"Claude API",target:{kind:"anthropic"}},
  {id:"qwen",name:"Qwen",title:"Qwen / 百炼 API",target:{kind:"qwenBeijing",workspaceId:""}},
  {id:"kimi",name:"Kimi",title:"Kimi API（中国站）",target:{kind:"kimiChina"}},
  {id:"a6api",name:"A6API",title:"A6API 中转",target:{kind:"a6Api"}},
  {id:"grok",name:"Grok",title:"xAI Grok API",target:{kind:"grok"}},
] satisfies {id:CredentialProvider;name:string;title:string;target:ProviderEndpoint}[];
type Entry=typeof providers[number];

function ProviderCard({entry,status,send,busy,connected}:{entry:Entry;status:ProviderConnection|undefined;send:(command:Command)=>Promise<boolean>;busy:boolean;connected:boolean}) {
  const [key,setKey]=useState(""),[target,setTarget]=useState<ProviderEndpoint>(entry.target),[workspace,setWorkspace]=useState(""),[pending,setPending]=useState(false);
  const lock=useRef(false);
  const saved=status?.keyConfigured===true,checking=status?.state==="checking",disabled=busy||pending||checking||!connected;
  const actual:ProviderEndpoint=target.kind==="qwenBeijing"?{kind:"qwenBeijing",workspaceId:workspace}:target;
  const valid=actual.kind!=="qwenBeijing"||/^[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$/.test(actual.workspaceId);
  async function act(command:Command){if(disabled||lock.current)return;lock.current=true;setPending(true);try{await send(command);}finally{lock.current=false;setPending(false);}}
  const label=checking?"正在读取模型列表":!saved?"尚未配置":status?.state==="listed"?"列表已读取；执行待验证":status?.state==="failed"?"检查未通过":"已加密保存，尚未验证";
  return <section className="panel" aria-labelledby={`${entry.id}-credential-heading`}>
    <div className="section-heading"><h2 id={`${entry.id}-credential-heading`}>{entry.title}</h2><span className="badge">{label}</span></div>
    <p className="secondary">单独保存此供应商的 Key。模型列表检查只读取目录，不发送编程任务；目录返回不代表模型生成、工具调用或额度已验证。</p>
    {entry.id==="a6api"&&<>
      <label htmlFor="a6api-origin">中转站地址（须与 Key 来源一致）</label>
      <select id="a6api-origin" disabled={disabled} value={target.kind} onChange={event=>{const kind=event.target.value;if(kind==="a6Api"||kind==="a6ApiVip")setTarget({kind});}}>
        <option value="a6Api">https://api.a6api.com/v1</option><option value="a6ApiVip">https://api.a6api.vip/v1</option>
      </select>
      <p className="secondary">只向所选地址读取模型列表，不自动切换域名。当前工作台保存一枚 A6API Key；更换地址前请核实凭证适用范围。</p>
    </>}
    <form onSubmit={event=>{event.preventDefault();if(disabled||lock.current)return;const value=key;setKey("");void act({type:"setProviderKey",provider:entry.id,key:value});}}>
      <label htmlFor={`${entry.id}-key`}>{entry.name} API Key</label>
      <input id={`${entry.id}-key`} type="password" autoComplete="off" spellCheck={false} value={key} onChange={event=>setKey(event.target.value)} minLength={16} maxLength={512} required disabled={disabled} placeholder={saved?"已保存；输入新 Key 可替换":"在本机粘贴 Key，无需发送到聊天"}/>
      <p className="secondary">按当前 Windows 用户及工作台目录加密；提交后清空输入，其他供应商的密钥保持独立。</p>
      <button className="primary" disabled={disabled}>加密保存 {entry.name} Key</button>
    </form>
    {entry.id==="qwen"&&<>
      <label htmlFor="qwen-region">百炼地域（须与 Key 一致）</label>
      <select id="qwen-region" disabled={disabled} value={target.kind} onChange={event=>{const kind=event.target.value;if(kind==="qwenBeijing")setTarget({kind,workspaceId:workspace});else if(kind==="qwenSingapore"||kind==="qwenHongKong")setTarget({kind});}}>
        <option value="qwenBeijing">北京</option><option value="qwenSingapore">新加坡</option><option value="qwenHongKong">中国香港</option>
      </select>
      {target.kind==="qwenBeijing"&&<><label htmlFor="qwen-workspace">北京业务空间 ID</label><input id="qwen-workspace" value={workspace} onChange={e=>setWorkspace(e.target.value)} maxLength={63} disabled={disabled} placeholder="从百炼控制台复制业务空间 ID"/></>}
      <p className="secondary">使用所选地域的按量 API Key；Coding Plan / Token Plan 另属专用接入。检查不会自动改用其他地域。</p>
    </>}
    <div className="card-footer">
      <button className="primary" disabled={disabled||!saved||!valid} onClick={()=>void act({type:"checkProviderModels",target:actual})}>读取 {entry.name} 模型列表</button>
      <button className="secondary-button" disabled={disabled||!saved} onClick={()=>void act({type:"removeProviderKey",provider:entry.id})}>删除本机 {entry.name} Key</button>
    </div>
    {status?.endpoint&&<p className="secondary" style={{overflowWrap:"anywhere"}}>本次检查端点：{status.endpoint}</p>}
    {status?.checkedAt&&<p className="secondary">检查时间：{new Date(status.checkedAt).toLocaleString("zh-CN",{hour12:false})}；重启后需重新检查。</p>}
    {status?.error&&<p role="alert"><ErrorText error={status.error}/></p>}
    {status?.state==="listed"&&<details><summary>已读取 {status.models.length} 个模型{status.listLimited?"（部分列表）":""}</summary>
      <p className="secondary">{status.listLimited?"达到当前页或展示上限，未返回的模型不能据此判定不可用。":"模型名称来自本次返回，执行能力仍需逐项验证。"}</p>
      {status.models.length?<ul>{status.models.map(id=><li key={id}>{id}</li>)}</ul>:<p>目录返回空列表，请核对账户及地域。</p>}
    </details>}
  </section>;
}
export function ProviderConnections({data,send,busy}:{data:Snapshot|null;send:(command:Command)=>Promise<boolean>;busy:boolean}){
  return <>{providers.map(entry=><ProviderCard key={entry.id} entry={entry} status={data?.providerConnections?.find(s=>s.provider===entry.id)} send={send} busy={busy} connected={!!data}/>)}</>;
}
