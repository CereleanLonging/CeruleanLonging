import type { ApiTarget, ApiWorkerSelection, ProjectWorkerAdapter } from "./contracts";

export const apiWorkerOptions = [
  ["openai", "OpenAI API"], ["anthropic", "Claude API"],
  ["deepseek", "DeepSeek API"], ["qwen", "Qwen / 百炼 API"],
  ["kimi", "Kimi API（中国站）"], ["grok", "xAI Grok API"], ["a6api", "A6API 中转（Chat Completions）"],
] satisfies [ProjectWorkerAdapter, string][];

export function newApiWorker(adapter: ProjectWorkerAdapter): ApiWorkerSelection | undefined {
  let target: ApiTarget;
  switch (adapter) {
    case "openai": target = {kind:"openAi"}; break;
    case "anthropic": target = {kind:"provider",endpoint:{kind:"anthropic"}}; break;
    case "deepseek": target = {kind:"provider",endpoint:{kind:"deepSeek"}}; break;
    case "qwen": target = {kind:"provider",endpoint:{kind:"qwenBeijing",workspaceId:""}}; break;
    case "kimi": target = {kind:"provider",endpoint:{kind:"kimiChina"}}; break;
    case "a6api": target = {kind:"provider",endpoint:{kind:"a6Api"}}; break;
    case "grok": target = {kind:"provider",endpoint:{kind:"grok"}}; break;
    default: return undefined;
  }
  return {target,model:"",fileTools:["read-files","write-files"],maxRounds:3,maxToolCalls:16,maxOutputTokens:4096,maxTotalTokens:30000,requestTimeoutSeconds:120};
}

export function ApiWorkerFields({worker,onChange,idPrefix="planning-api"}:{idPrefix?:string;worker:ApiWorkerSelection;onChange:(worker:ApiWorkerSelection)=>void}) {
  const endpoint=worker.target.kind==="provider"?worker.target.endpoint:undefined;
  const qwen=endpoint?.kind==="qwenBeijing"||endpoint?.kind==="qwenSingapore"||endpoint?.kind==="qwenHongKong";
  const limits=[
    ["maxRounds","每次尝试最多 API 轮次",128],
    ["maxToolCalls","每次尝试最多文件工具调用",128],
    ["maxOutputTokens","每轮输出 token 上限",4294967295],
    ["maxTotalTokens","每次尝试累计 token 停止阈值",10000000],
    ["requestTimeoutSeconds","单个 API 请求时限（秒）",600],
  ] as const;
  return <section className="panel" aria-label="API 编程执行配置">
    <h3>API 编程执行配置</h3>
    {(endpoint?.kind==="a6Api"||endpoint?.kind==="a6ApiVip")&&<>
      <label htmlFor={`${idPrefix}-relay-origin`}>A6API 中转站地址</label>
      <select id={`${idPrefix}-relay-origin`} value={endpoint.kind} onChange={e=>{const kind=e.target.value;if(kind==="a6Api"||kind==="a6ApiVip")onChange({...worker,target:{kind:"provider",endpoint:{kind}}});}}>
        <option value="a6Api">https://api.a6api.com/v1</option><option value="a6ApiVip">https://api.a6api.vip/v1</option>
      </select>
      <p className="secondary">须与接入管理中的 Key 来源一致。地址随方案一同批准，不自动切换域名。</p>
    </>}
    {qwen&&<>
      <label htmlFor={`${idPrefix}-region`}>百炼地域</label>
      <select id={`${idPrefix}-region`} value={endpoint.kind} onChange={e=>{
        const kind=e.target.value;
        if(kind==="qwenBeijing")onChange({...worker,target:{kind:"provider",endpoint:{kind,workspaceId:""}}});
        else if(kind==="qwenSingapore"||kind==="qwenHongKong")onChange({...worker,target:{kind:"provider",endpoint:{kind}}});
      }}><option value="qwenBeijing">北京</option><option value="qwenSingapore">新加坡</option><option value="qwenHongKong">中国香港</option></select>
      {endpoint.kind==="qwenBeijing"&&<><label htmlFor={`${idPrefix}-workspace`}>北京业务空间 ID</label><input id={`${idPrefix}-workspace`} required maxLength={63} pattern="[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?" value={endpoint.workspaceId} onChange={e=>onChange({...worker,target:{kind:"provider",endpoint:{kind:"qwenBeijing",workspaceId:e.target.value}}})}/></>}
      <p className="secondary">地域须与接入管理中保存的按量 API Key 一致；更改地域或业务空间需重新规划和批准。</p>
    </>}
    <label htmlFor={`${idPrefix}-files`}>模型文件权限</label>
    <select id={`${idPrefix}-files`} value={worker.fileTools.includes("write-files")?(worker.fileTools.includes("read-files")?"readWrite":"write"):"read"} onChange={e=>onChange({...worker,writeScope:e.target.value==="read"?undefined:worker.writeScope,fileTools:e.target.value==="read"?["read-files"]:e.target.value==="write"?["write-files"]:["read-files","write-files"]})}>
      <option value="readWrite">读取与写入项目文件</option><option value="read">只读项目文件</option><option value="write">只写项目文件</option>
    </select>
    {worker.fileTools.includes("write-files")&&<><label htmlFor={`${idPrefix}-write-scope`}>允许写入的文件</label>
      <select id={`${idPrefix}-write-scope`} value={worker.writeScope??"workspace"} onChange={e=>onChange({...worker,writeScope:e.target.value==="declared-artifacts"?"declared-artifacts":undefined})}>
        <option value="workspace">项目目录内文件</option><option value="declared-artifacts">仅当前任务的交付文件</option>
      </select>
      {worker.writeScope==="declared-artifacts"&&<p className="secondary">写入必须使用方案中列出的准确文件路径；读取依赖文件的权限不受影响。修改范围后须重新规划和批准。</p>}</>}
    <p className="secondary">文件操作限于批准的项目目录。验收命令由独立验收器执行。</p>
    <div className="planning-fields">{limits.map(([field,label,max])=><div key={field}><label htmlFor={`${idPrefix}-${field}`}>{label}</label><input id={`${idPrefix}-${field}`} type="number" min={1} max={max} required value={worker[field]} onChange={e=>onChange({...worker,[field]:Number(e.target.value)})}/></div>)}</div>
    <p className="secondary">每轮计入项目编程请求上限。累计 token 按供应商回复后的用量统计，最后一轮可能超过停止阈值；这不是金额硬上限。</p>
    <p className="secondary">输出长度须符合所选模型的供应商限制；如需最大输出，请填写该模型公布的最大值。实际请求还受本次尝试剩余 token 预算约束。</p>
    <p role="status">保存配置并批准方案后，可启动持续执行。编程 API 仅使用已批准的文件工具。</p>
  </section>;
}
