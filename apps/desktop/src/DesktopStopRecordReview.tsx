import { ErrorText } from "./ErrorText";
import { useEffect, useRef, useState } from "react";
import type { Command, DiagnosticFile } from "./contracts";
import "./CodexJobs.css";
export function DesktopStopRecordReview({file,busy,send}:{file:DiagnosticFile;busy:boolean;send:(command:Command)=>Promise<boolean>}) {
  const [review,setReview]=useState<DiagnosticFile|null>(null),[reason,setReason]=useState(""),[ack,setAck]=useState(false),[pending,setPending]=useState(false),[message,setMessage]=useState("");
  const lock=useRef(false),dialog=useRef<HTMLDialogElement>(null);
  const blocked=busy||pending;
  const valid=!!review?.reviewDigest&&review.reviewDigest===file.reviewDigest&&review.relativePath===file.relativePath&&file.reviewKind==="desktop-stop"&&!file.reviewed;
  const text=reason.trim(),validReason=!!text&&!text.includes("\0")&&new TextEncoder().encode(text).length<=8000;
  useEffect(()=>{if(!review||!dialog.current)return;const el=dialog.current,opener=document.activeElement;el.showModal();return()=>{el.close();if(opener instanceof HTMLElement&&opener.isConnected)opener.focus();};},[review]);
  async function confirm(){if(blocked||lock.current||!valid||!review||!validReason||!ack)return;lock.current=true;setPending(true);try{if(await send({type:"reviewDesktopStopRecord",relativePath:review.relativePath,evidenceDigest:review.reviewDigest!,reason:text})){setReview(null);setMessage("复核已保存，原文件保留；请重新启动并校验。原尝试结果仍未知，仅恢复本地管理。");}else setMessage("复核未确认完成，请刷新后核对。不会发送任务或停止动作。");}catch{setMessage("连接中断，请刷新后核对。");}finally{lock.current=false;setPending(false);}}
  return <>
    {file.reviewed?<p>已复核并保留原文；重新启动后仍须通过原批准链校验。原尝试不能继续，只能核对后结束本地跟踪。</p>:file.reviewKind==="desktop-stop"&&file.reviewDigest?<button className="secondary-button" disabled={blocked} onClick={()=>{setReview(file);setReason("");setAck(false);setMessage("");}}>复核未写完整的桌面停止记录…</button>:<p>此记录尚不满足复核条件，请保留原证据继续核查。</p>}
    {message&&!review&&<p role="status">{message}</p>}
    <dialog ref={dialog} className="panel modal unsent-assignment-dialog" aria-label="桌面停止残片复核" onCancel={e=>{e.preventDefault();if(!blocked)setReview(null);}}>{review&&<>
      <h2>复核未写完整的桌面停止记录</h2>
      <p style={{overflowWrap:"anywhere"}}>{review.relativePath}</p>
      <p>保留原文件，依据原批准、派发和有效停止前置记录另存复核凭据。复核只恢复本地管理，不会补发任务或停止动作，不代表取消成功或通过验收。重新启动校验后，原尝试仍待核对，只能在自行处理外部应用后结束本地跟踪；不能继续原尝试。</p>
      <label htmlFor="desktop-stop-reason">复核说明</label><textarea id="desktop-stop-reason" rows={3} value={reason} disabled={blocked} onChange={e=>setReason(e.target.value)}/>
      <label className="checkbox-label"><input type="checkbox" checked={ack} disabled={blocked} onChange={e=>setAck(e.target.checked)}/>我已核对原记录，理解复核只恢复本地管理，原尝试不能继续，结果仍未知</label>
      {!valid&&<p role="alert">记录已变化，请返回刷新诊断。</p>}{message&&<p role="alert"><ErrorText error={message}/></p>}
      <div className="modal-actions"><button className="secondary-button" disabled={blocked} onClick={()=>setReview(null)}>返回检查</button><button className="primary" disabled={blocked||!valid||!validReason||!ack} onClick={()=>void confirm()}>保存桌面停止复核</button></div>
    </>}</dialog>
  </>;
}
