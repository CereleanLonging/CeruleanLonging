import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { ApiModelPicker } from "./ApiModelPicker";
import type { ApiModelAssignment } from "./apiModels";
import { useRef, useState } from "react";
import type { Command, OpenAiConnection as Status } from "./contracts";
export function OpenAiConnection({status,send,busy,connected,view="all",idPrefix="openai",modelAssignments}:{modelAssignments?:ApiModelAssignment[];status:Status|undefined;send:(command:Command,onError?:(message:string)=>void)=>Promise<boolean>;busy:boolean;connected:boolean;view?:"all"|"api"|"key";idPrefix?:string}) {
  const [key,setKey]=useState("");
  const [pending,setPending]=useState(false);
  const [action,setAction]=useState<"save"|"remove"|"check"|null>(null);
  const [actionError,setActionError]=useState("");
  const [modelRequest,setModelRequest]=useState(0);
  const lock=useRef(false);
  const checking=status?.state==="checking";
  const disabled=busy||pending||checking||!connected;
  async function act(command: Command) {
    if (disabled || lock.current) return;
    lock.current = true;
    setPending(true);
    setActionError("");
    setAction(command.type === "setOpenAiKey" ? "save"
      : command.type === "removeOpenAiKey" ? "remove" : "check");
    try {
      let reportedError = false;
      const accepted = await send(command, message => { reportedError = true; setActionError(message); });
      if (!accepted) setActionError(message => message || "操作未完成，请核对当前状态与页面提示。");
      if (accepted && !reportedError && command.type !== "removeOpenAiKey") setModelRequest(value => value + 1);
    } catch (error) {
      setActionError(String(error));
    } finally {
      lock.current = false;
      setPending(false);
    }
  }
  const labels:Record<string,string>={notConfigured:"尚未配置",unchecked:"已加密保存，待检查",checking:"正在检查官方 API",available:"官方 API 认证通过",failed:"连接检查未通过"};
  return <section className="panel">
    <div className="section-heading"><HelpHeading as="h3" topic="OpenAiConnection">OpenAI {view==="key"?"Key":"API"}</HelpHeading><span className="badge">{labels[status?.state??"notConfigured"]??"尚未配置"}</span></div>
    <p className="secondary">使用官方 API Key。API 用量与 Codex 订阅分开计费；角色能力由项目配置和已批准方案决定。</p>
    {view!=="api"&&<form onSubmit={event=>{event.preventDefault();if(disabled||lock.current)return;const value=key;setKey("");void act({type:"setOpenAiKey",key:value});}}>
      <label htmlFor={`${idPrefix}-key`}>OpenAI API Key</label>
      <input id={`${idPrefix}-key`} disabled={disabled} type="password" autoComplete="off" spellCheck={false} value={key} onChange={event=>setKey(event.target.value)} placeholder={status?.keyConfigured?"已保存；输入新 Key 可替换":"在本机粘贴 Key，无需发送到聊天"} minLength={16} maxLength={512} required />
      <p className="secondary">以当前 Windows 用户加密保存。不会在这里回显；更换用户或迁移工作台数据目录后需重新配置。</p>
      <div className="integration-actions credential-actions">
        <button type="submit" className="primary" disabled={disabled}>{pending&&action==="save"?"正在保存…":"加密保存 Key"}</button>
        <button type="button" className="secondary-button" disabled={disabled||!status?.keyConfigured} onClick={()=>void act({type:"removeOpenAiKey"})}>{pending&&action==="remove"?"正在删除…":"删除本机 Key"}</button>
      </div>
      {!pending&&!actionError&&action==="save"&&<p role="status">{status?.keyConfigured?"OpenAI Key 已加密保存，可以测试连通。":"保存操作已接收，凭证状态尚未确认，请重新读取状态。"}</p>}
      {!pending&&!actionError&&action==="remove"&&<p role="status">{status?.keyConfigured?"删除操作已接收，凭证状态尚未确认，请重新读取状态。":"OpenAI Key 已从本机删除。"}</p>}
    </form>}
    {actionError&&<p role="alert"><ErrorText error={actionError}/></p>}
    <div className="connection-test">
      <button type="button" className="primary" disabled={disabled||!status?.keyConfigured} onClick={()=>void act({type:"checkOpenAi"})}>{checking||(pending&&action==="check")?"测试中…":"测试连通"}</button>
      <div className="connection-test-feedback">
    <p className="secondary">验证 OpenAI API 认证与模型目录访问。</p>
    {!status?.keyConfigured&&<p className="secondary">请先保存 OpenAI Key，再测试连通。</p>}
    {checking&&<p role="status">正在验证 OpenAI API 认证和模型目录…</p>}
    {!pending&&status?.state==="available"&&<p role="status">测试通过 · OpenAI API 认证和模型目录读取成功。</p>}
    {status?.error && <p role="alert">测试未通过 · <ErrorText error={status.error}/></p>}
      </div>
    </div>
    {modelAssignments && <ApiModelPicker target={{kind:"openAi"}} assignments={modelAssignments} models={status?.models??[]}
      keyConfigured={status?.keyConfigured===true} ready={status?.state==="available"} checking={checking} disabled={disabled} valid
      failed={status?.state==="failed"} request={modelRequest} autoPrompt={view==="key"} onCheck={()=>void act({type:"checkOpenAi"})}/>}
    {view!=="key"&&<>
      {!!status?.models.length && <details><summary>可见模型（{status.models.length}）</summary><p>{status.models.join("、")}</p></details>}
    </>}
  </section>;
}
