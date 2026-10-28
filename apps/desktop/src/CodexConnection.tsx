import { useState } from "react";
import type { Command, CodexConnectionCheck } from "./contracts";

export function CodexConnection({status,send,busy,connected}:{status:CodexConnectionCheck|null;send:(command:Command)=>Promise<boolean>;busy:boolean;connected:boolean}) {
  const [executable,setExecutable]=useState(status?.executable ?? "");
  const checking=status?.state==="checking";
  const label=status ? ({checking:"正在检查连接",available:"连接检查通过",loginRequired:"需要在 Codex 中登录",failed:"连接检查未通过"}[status.state] ?? "状态待核对") : "尚未检查";
  return <section className="panel codex-connection" aria-label="Codex 连接检查">
    <div className="section-heading"><h2>Codex Agent</h2><span className="badge">{label}</span></div>
    <p className="secondary">使用本机 Codex 已有登录，检查协议连接和可用模型。本操作不发送编程任务。</p>
    <form onSubmit={async event=>{event.preventDefault();await send({type:"checkCodex",executable:executable.trim()});}}>
      <label htmlFor="codex-executable">Codex 程序完整路径</label><input id="codex-executable" required value={executable} onChange={event=>setExecutable(event.target.value)} placeholder="C:\\…\\codex.exe" />
      <button className="primary" disabled={!connected || busy || checking}>{checking ? "检查中…" : "检查现有 Codex 登录"}</button>
    </form>
    {status?.serverVersion && <p className="secondary">{status.serverVersion}</p>}
    {status?.accountType && <p>登录类型：{status.accountType}</p>}
    {status?.models.length ? <p>可用模型：{status.models.join("、")}</p> : null}
    {status?.error && <p role="alert">{status.error}</p>}
    {status && <p className="secondary">检查时间：{new Date(status.checkedAt).toLocaleString("zh-CN")}。这是当次连接结果，编程执行和指挥权限尚待验收。</p>}
  </section>;
}
