import { ErrorText } from "./ErrorText";
import { useEffect, useRef, useState } from "react";
import type { Command, DiagnosticFile } from "./contracts";
import "./CodexJobs.css";
export function ApiPublicationReview({file,busy,send}:{file:DiagnosticFile;busy:boolean;send:(command:Command)=>Promise<boolean>}) {
  const [review,setReview]=useState<DiagnosticFile|null>(null),[reason,setReason]=useState(""),[ack,setAck]=useState(false),[pending,setPending]=useState(false),[message,setMessage]=useState("");
  const lock=useRef(false),dialog=useRef<HTMLDialogElement>(null);
  const stopped=file.relativePath.startsWith("api-cancellations/"),workspace=file.relativePath.startsWith("api-workspace-bindings/"),blocked=busy||pending;
  const valid=!!review?.reviewDigest&&review.reviewDigest===file.reviewDigest&&review.relativePath===file.relativePath&&file.reviewKind==="api-publication"&&!file.reviewed;
  const text=reason.trim(),validReason=!!text&&!text.includes("\0")&&new TextEncoder().encode(text).length<=8000;
  useEffect(()=>{if(!review||!dialog.current)return;const el=dialog.current,opener=document.activeElement;el.showModal();return()=>{el.close();if(opener instanceof HTMLElement&&opener.isConnected)opener.focus();};},[review]);
  async function confirm(){if(blocked||lock.current||!valid||!review||!validReason||!ack)return;lock.current=true;setPending(true);try{if(await send({type:"reviewApiPublication",relativePath:review.relativePath,evidenceDigest:review.reviewDigest!,reason:text})){setReview(null);setMessage("复核已保存，原文件保留；请重新启动并校验。任务结果仍须核对。");}else setMessage("复核未确认完成，请刷新后核对。不会重新发送请求。");}catch{setMessage("连接中断，请刷新后核对。");}finally{lock.current=false;setPending(false);}}
  return <>
    {file.reviewed?<p>已复核并保留原文；复核不代表模型执行成功或供应商确认取消。重新启动后仍须通过关联校验。</p>:file.reviewKind==="api-publication"&&file.reviewDigest?<button className="secondary-button" disabled={blocked} onClick={()=>{setReview(file);setReason("");setAck(false);setMessage("");}}>复核未写完整的 API 记录…</button>:<p>此记录尚不满足复核条件，请保留原证据继续核查。</p>}
    {message&&!review&&<p role="status">{message}</p>}
    <dialog ref={dialog} className="panel modal unsent-assignment-dialog" aria-label="API 中断记录复核" onCancel={e=>{e.preventDefault();if(!blocked)setReview(null);}}>{review&&<>
      <h2>{stopped?"复核未写完整的 API 停止记录":workspace?"复核未写完整的工作目录准备记录":"复核未写完整的 API 启动记录"}</h2>
      <p style={{overflowWrap:"anywhere"}}>{review.relativePath}</p>
      <p>{stopped?"将保留原文和原启动身份，另存复核说明。不会重新发送停止或模型请求，也不代表供应商已取消、执行成功或免计费。":workspace?"将保留原准备记录、启动身份和已有文件，另存复核说明。不会重建工作目录、补写就绪证明或启动模型。":"将保留原文并永久保留该启动编号，另存复核说明。无法确认的项目或尝试身份不会被猜测补全；不会启动模型或重发请求。"}重新启动后仍须通过全部关联校验，执行结果需另行核对。</p>
      <label htmlFor="api-publication-reason">复核说明</label><textarea id="api-publication-reason" rows={3} value={reason} disabled={blocked} onChange={e=>setReason(e.target.value)}/>
      <label className="checkbox-label"><input type="checkbox" checked={ack} disabled={blocked} onChange={e=>setAck(e.target.checked)}/>我已核对原记录，理解复核不会完成任务或确认供应商取消</label>
      {!valid&&<p role="alert">记录已变化，请返回刷新诊断。</p>}{message&&<p role="alert"><ErrorText error={message}/></p>}
      <div className="modal-actions"><button className="secondary-button" disabled={blocked} onClick={()=>setReview(null)}>返回检查</button><button className="primary" disabled={blocked||!valid||!validReason||!ack} onClick={()=>void confirm()}>保存 API 复核</button></div>
    </>}</dialog>
  </>;
}
