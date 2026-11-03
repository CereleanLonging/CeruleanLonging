import { ErrorText } from "./ErrorText";
import { useEffect, useRef, useState } from "react";
import type { Command, DiagnosticFile } from "./contracts";
import "./CodexJobs.css";

const impacts:Record<string,string>={
  "0":"保留残片，并永久禁止复用此管理操作编号。不会补造请求内容或调用模型。",
  "1":"模型可能已经被调用。结果仍保持未知，不能重新发送请求；重开后请核对并明确处置原请求。",
  "2":"不会执行缺失的分配或应用。重开后须通过原入口核对关联启动并确认关闭，原决定不能再次派发。",
  "3":"只有与真实工作流事件对应的回执才能重建；不会重新执行命令或把任务标为完成。",
  "4":"不会猜测丢失的用户说明。重开后请重新核对并确认；已有完整持久关闭证据的关联记录可按原规则核对恢复。"
};
export function ManagementRecordReview({file,busy,send}:{file:DiagnosticFile;busy:boolean;send:(command:Command)=>Promise<boolean>}){
  const [review,setReview]=useState<DiagnosticFile|null>(null),[reason,setReason]=useState(""),[ack,setAck]=useState(false),[pending,setPending]=useState(false),[message,setMessage]=useState("");
  const lock=useRef(false),dialog=useRef<HTMLDialogElement>(null);
  const stage=/^management-decisions\/[a-zA-Z0-9_-]+\.([0-4])\./.exec(file.relativePath)?.[1]??"";
  const valid=!!review?.reviewDigest&&review.reviewDigest===file.reviewDigest&&file.reviewKind==="management"&&!file.reviewed;
  const text=reason.trim(),validReason=!!text&&!text.includes("\0")&&new TextEncoder().encode(text).length<=8000;
  const blocked=busy||pending;
  useEffect(()=>{if(!review||!dialog.current)return;const el=dialog.current,opener=document.activeElement;el.showModal();return()=>{el.close();if(opener instanceof HTMLElement&&opener.isConnected)opener.focus();};},[review]);
  async function confirm(){
    if(blocked||lock.current||!review||!valid||!validReason||!ack)return;
    lock.current=true;setPending(true);setMessage("");
    try{if(await send({type:"reviewManagementRecord",relativePath:review.relativePath,evidenceDigest:review.reviewDigest!,reason:text})){setReview(null);setMessage("复核已保存，原文保留。请重新启动并校验，再核对原请求的后续处置。");}else setMessage("复核未确认完成，请返回刷新后核对。不会自动重发请求。");}
    catch{setMessage("连接中断，请返回刷新后核对复核记录。");}
    finally{lock.current=false;setPending(false);}
  }
  return <>
    {file.reviewed?<p>已保存复核，原文保留；仍须重新启动并通过全部关联校验。</p>:file.reviewKind==="management"&&file.reviewDigest?<button className="secondary-button" disabled={blocked} onClick={()=>{setReview(file);setReason("");setAck(false);setMessage("");}}>复核半写管理记录…</button>:<p>此文件尚不满足复核条件。请保留原证据，核对前置记录和关联冲突。</p>}
    {message&&!review&&<p role="status">{message}</p>}
    <dialog ref={dialog} className="panel modal unsent-assignment-dialog" aria-labelledby="management-record-review-title" onCancel={e=>{e.preventDefault();if(!blocked)setReview(null);}}>{review&&<>
      <h2 id="management-record-review-title">确认复核半写管理记录</h2><p style={{overflowWrap:"anywhere"}}>{review.relativePath}</p>
      <p>{impacts[stage]}</p><p>原始残片保留，本次说明另存。保存复核不代表任务完成；存在坏关联或其他未决文件时，重启后仍会停在诊断页面。</p>
      <label htmlFor="management-record-review-reason">本次复核说明</label><textarea id="management-record-review-reason" rows={3} value={reason} onChange={e=>setReason(e.target.value)} disabled={blocked}/>
      <label className="checkbox-label"><input type="checkbox" checked={ack} onChange={e=>setAck(e.target.checked)} disabled={blocked}/>我已核对目标和本阶段的处置影响</label>
      {!valid&&<p role="alert">记录已变化，请返回刷新后重新复核。</p>}{message&&<p role="alert"><ErrorText error={message}/></p>}
      <div className="modal-actions"><button className="secondary-button" disabled={blocked} onClick={()=>setReview(null)}>返回检查</button><button className="primary" disabled={blocked||!valid||!validReason||!ack} onClick={()=>void confirm()}>保存管理复核</button></div>
    </>}</dialog>
  </>;
}
