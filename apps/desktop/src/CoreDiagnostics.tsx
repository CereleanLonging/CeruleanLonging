import { useRef, useState } from "react";
import type { Command, Snapshot } from "./contracts";
import { StartupIntentReview } from "./StartupIntentReview";
import { ManagementRecordReview } from "./ManagementRecordReview";
import { DesktopStopRecordReview } from "./DesktopStopRecordReview";
import { ApiPublicationReview } from "./ApiPublicationReview";
const states:Record<string,string>={empty:"空文件","truncated-json":"内容未写完整","invalid-json":"格式无效","complete-json-unverified":"内容完整，尚未通过关联校验",oversized:"超过诊断读取上限",unreadable:"无法读取","not-ordinary-file":"非普通文件或不可读取"};
const managementStages:Record<string,string>={"0":"管理请求意图","1":"模型响应或未知结果","2":"分配或应用准备","3":"应用回执","4":"用户处置记录"};
function managementStage(path:string){const match=/^management-decisions\/[a-zA-Z0-9_-]+\.([0-4])\.(?:json|pending)$/.exec(path);return match?managementStages[match[1]]:"名称未识别的管理文件";}
const desktopStages:Record<string,string>={intent:"原任务派发意图",proof:"原任务完成回复","seal-intent":"文件封存意图",completion:"原交付封存","stop-request":"用户停止请求","stop-intent":"单次停止动作意图","stop-proof":"取消终态证明"};
function desktopStage(path:string){const match=/^desktop-task-dispatch-v1\/[0-9a-f]{64}\/([a-z-]+)\.(?:json|pending)$/.exec(path);return match?desktopStages[match[1]]??"名称未识别的桌面记录":"名称未识别的桌面记录";}
export function CoreDiagnostics({data,busy,error,refresh,restart,send}:{data:Snapshot;busy:boolean;error:string;refresh:()=>Promise<unknown>;restart:()=>Promise<boolean>;send:(command:Command)=>Promise<boolean>}) {
  const [working,setWorking]=useState(false),[notice,setNotice]=useState("");const lock=useRef(false);
  const diagnostic=data.coreDiagnostic;if(!diagnostic)return null;
  async function recheck(){if(busy||working||lock.current)return;lock.current=true;setWorking(true);setNotice("");try{if(await restart())setNotice("已重新校验。若仍显示此页面，说明数据尚未恢复。");}finally{lock.current=false;setWorking(false);}}
  return <section className="panel" aria-label="执行数据诊断">
    <h1>执行数据需要恢复</h1><p>核心未加载执行任务，当前提供诊断与专门的记录复核。普通修改、派发和模型调用均已禁用；原始证据保留。</p>
    <p role="alert">{diagnostic.message}</p><p>数据目录：{data.dataDirectory}</p>
    {!!diagnostic.desktopFiles?.length&&<section aria-label="桌面任务记录诊断"><h2>桌面派发、停止与封存记录</h2><p>按记录目录分组显示原文件位置、写入阶段与摘要。同一目录对应待核对的尝试，项目和任务身份仍须通过原批准链确认。内容完整不代表任务完成或取消成功。</p><p>符合原批准与派发证据的停止残片可另存复核，恢复本地管理；其他残片仍须继续核查。原文件保留，重新校验不会补发任务或停止动作。</p>
      {diagnostic.desktopFiles.map(file=><article className="panel" key={file.relativePath}><h3 style={{overflowWrap:"anywhere"}}>{file.relativePath}</h3><p>{desktopStage(file.relativePath)} · {states[file.state]??file.state}{file.bytes!==null?` · ${file.bytes} 字节`:""}</p>{file.sha256&&<details><summary>查看桌面文件摘要</summary><p style={{overflowWrap:"anywhere"}}>SHA-256：{file.sha256}</p></details>}<DesktopStopRecordReview file={file} busy={busy||working} send={send}/></article>)}
    </section>}
    {!!diagnostic.apiFiles?.length&&<section aria-label="API 记录诊断"><h2>API 启动与停止记录</h2><p>只展示摘要。记录完整仍须核对原项目与分配关系；不会重新发送请求。</p>{diagnostic.apiFiles.map(file=><article className="panel" key={file.relativePath}><h3 style={{overflowWrap:"anywhere"}}>{file.relativePath}</h3><p>{states[file.state]??file.state}{file.bytes!==null?` · ${file.bytes} 字节`:""}</p>{file.sha256&&<details><summary>原文件摘要</summary><p style={{overflowWrap:"anywhere"}}>{file.sha256}</p></details>}<ApiPublicationReview file={file} busy={busy||working} send={send}/></article>)}</section>}
    {!!diagnostic.managementFiles.length&&<section aria-label="管理记录诊断"><h2>管理记录</h2><p>以下按文件名标示写入阶段，只显示文件摘要。内容完整不代表关联有效，清单中的文件也不一定都是故障来源。</p>
      {diagnostic.managementFiles.map(file=><article className="panel" key={file.relativePath}><h3 style={{overflowWrap:"anywhere"}}>{file.relativePath}</h3><p>{managementStage(file.relativePath)} · {states[file.state]??file.state}{file.bytes!==null?` · ${file.bytes} 字节`:""}</p>{file.sha256&&<details><summary>查看管理文件摘要</summary><p style={{overflowWrap:"anywhere"}}>SHA-256：{file.sha256}</p></details>}<ManagementRecordReview file={file} busy={busy||working} send={send}/></article>)}
    </section>}
    <p>以下仅列出发现的未完成启动文件，不代表全部故障原因。请勿直接删除或覆盖这些记录。</p>
    {diagnostic.startupFiles.map(file=><article className="panel" key={file.relativePath}><h3 style={{overflowWrap:"anywhere"}}>{file.relativePath}</h3><p>{states[file.state]??file.state}{file.bytes!==null?` · ${file.bytes} 字节`:""}</p>{file.sha256&&<details><summary>查看原文件摘要</summary><p style={{overflowWrap:"anywhere"}}>SHA-256：{file.sha256}</p></details>}<StartupIntentReview file={file} busy={busy||working} send={send}/></article>)}
    {!diagnostic.desktopFiles?.length&&!diagnostic.apiFiles?.length&&!diagnostic.startupFiles.length&&!diagnostic.managementFiles.length&&<p>当前未发现可列出的执行记录，需要进一步核查上方错误对应的记录。</p>}
    {diagnostic.inventoryLimited&&<p>部分目录无法检查或已达到读取限制，诊断清单不完整。</p>}
    <p>完成有证据支持的数据恢复后，可以重新启动并校验。校验通过后会继续既有授权流程，未决任务仍须按原恢复规则核对。</p>
    {error&&<p role="alert">{error}</p>}{notice&&<p role="status">{notice}</p>}
    <div className="modal-actions"><button className="secondary-button" disabled={busy||working} onClick={()=>void refresh()}>刷新诊断</button><button className="primary" disabled={busy||working} onClick={()=>void recheck()}>重新启动并校验</button></div>
  </section>;
}
