import { preferenceKey, readPreference, useAccessPreference } from "./accessPreferences";
import { HelpHeading } from "./HelpHeading";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Command, CodexConnectionCheck } from "./contracts";
import { ErrorText } from "./ErrorText";

type CodexCandidate = { executable: string; directory: string; source: string };
type CodexScan = { candidates: CodexCandidate[]; warnings: string[]; limited: boolean };

export function CodexConnection({status,send,busy,connected,scope,value,onChange,idPrefix="codex"}:{value?:string;onChange?:(value:string)=>void;idPrefix?:string;scope?:string;status:CodexConnectionCheck|null;send:(command:Command)=>Promise<boolean>;busy:boolean;connected:boolean}) {
  const [savedExecutable,setSavedExecutable,preferenceError]=useAccessPreference(scope,"codex-executable",status?.executable??"");
  const executable=value??savedExecutable;
  function setExecutable(next:string) {
    if(onChange)onChange(next);
    else setSavedExecutable(next);
  }
  const [scan,setScan]=useState<CodexScan|null>(null);
  const [scanError,setScanError]=useState("");
  const [scanning,setScanning]=useState(false);
  const [notice,setNotice]=useState("");
  const lock=useRef(false);
  const edited=useRef(readPreference(preferenceKey(scope,"codex-executable"))!==undefined);
  useEffect(()=>{if(value===undefined&&!edited.current&&status?.executable)setExecutable(status.executable);},[status?.executable]);
  const checking=status?.state==="checking";
  const disabled=busy||checking||scanning;
  const desktop="__TAURI_INTERNALS__" in window;
  async function discover() {
    if(disabled||lock.current||!desktop)return;
    lock.current=true;setScanning(true);setScanError("");setScan(null);setNotice("");
    try { setScan(await invoke<CodexScan>("scan_codex_installations",{hint:executable.trim()||null})); }
    catch(error) { setScanError(String(error)); }
    finally {lock.current=false;setScanning(false);}
  }
  const samePath=!!status&&status.executable.replaceAll("/","\\").toLowerCase()===executable.trim().replaceAll("/","\\").toLowerCase();
  const label=status ? (!samePath?"路径已更改，需重新检查":({checking:"正在检查连接",available:"连接检查通过",loginRequired:"需要在 Codex 中登录",failed:"连接检查未通过"}[status.state] ?? "状态待核对")) : "尚未检查";
  return <section className="panel codex-connection" aria-label="Codex 连接检查">
    <div className="section-heading"><HelpHeading as="h2" topic="CodexConnection">Codex Agent</HelpHeading><span className="badge">{label}</span></div>
    <p className="secondary">使用本机 Codex 已有登录，检查协议连接和可用模型。本操作不发送编程任务。</p>
    {preferenceError&&<p role="alert">程序路径自动保存失败，输入仍保留；请重新填写后重试。</p>}
    <form onSubmit={async event=>{event.preventDefault();if(disabled||lock.current||!connected)return;lock.current=true;setNotice("");try{await send({type:"checkCodex",executable:executable.trim()});}finally{lock.current=false;}}}>
      <label htmlFor={`${idPrefix}-executable`}>Codex 程序完整路径</label>
      <div className="codex-path-row">
        <input id={`${idPrefix}-executable`} required disabled={disabled} value={executable} onChange={event=>{edited.current=true;setExecutable(event.target.value);setNotice("");}} placeholder="C:\\…\\codex.exe" aria-describedby={`${idPrefix}-path-help`} />
        <button type="button" className="secondary-button" disabled={disabled||!desktop} onClick={()=>void discover()}>{scanning ? "正在扫描…" : "扫描本地 Codex"}</button>
      </div>
      <p id={`${idPrefix}-path-help`} className="secondary">请选择具体的 codex.exe 文件。若只知道安装文件夹，可先填入该目录，再扫描查找。</p>
      <div className="integration-actions codex-check-actions">
        <button type="submit" className="primary" disabled={!connected||disabled||!executable.trim()}>{checking ? "检查中…" : "检查现有 Codex 登录"}</button>
      </div>
    </form>
    <p className="secondary">扫描正在运行的 Codex、PATH 和常见安装目录，仅查找程序位置。选用路径后，再点击登录检查。</p>
    {!desktop&&<p className="secondary">请在桌面应用中使用本机扫描。</p>}
    {scanError&&<p role="alert"><ErrorText error={scanError}/></p>}
    {scan&&<div className="codex-scan-results" aria-label="Codex 扫描结果">
      <p role="status">{scan.candidates.length?`找到 ${scan.candidates.length} 个候选程序，请确认要使用的路径。`:"本次扫描未找到可用程序。可先打开 Codex 后重新扫描，或填写具体安装目录。"}</p>
      {scan.warnings.map((warning,i)=><p key={i} className="secondary">{warning}</p>)}
      {scan.candidates.map(candidate=><div className="codex-scan-candidate" key={candidate.executable}>
        <p><strong>{candidate.source}</strong></p><code>{candidate.executable}</code>
        <button type="button" className="secondary-button" disabled={disabled} onClick={()=>{edited.current=true;setExecutable(candidate.executable);setNotice("已填入所选程序路径，请点击“检查现有 Codex 登录”。");}}>使用此路径</button>
      </div>)}
    </div>}
    {notice&&<p role="status">{notice}</p>}
    {samePath&&status?.serverVersion && <p className="secondary">{status.serverVersion}</p>}
    {samePath&&status?.accountType && <p>登录类型：{status.accountType}</p>}
    {samePath&&status?.models.length ? <p>可用模型：{status.models.join("、")}</p> : null}
    {samePath&&status?.error && <p role="alert"><ErrorText error={status.error}/></p>}
    {samePath&&status && <p className="secondary">检查时间：{new Date(status.checkedAt).toLocaleString("zh-CN")}。这是当次连接结果，编程执行和指挥权限尚待验收。</p>}
    {status&&!samePath&&<p className="secondary">当前路径尚未检查，上次连接结果属于其他程序路径。</p>}
  </section>;
}
