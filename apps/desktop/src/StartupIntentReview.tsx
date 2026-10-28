import { useEffect, useRef, useState } from "react";
import type { Command, DiagnosticFile } from "./contracts";
import "./CodexJobs.css";

export function StartupIntentReview({file,busy,send}:{file:DiagnosticFile;busy:boolean;send:(command:Command)=>Promise<boolean>}) {
  const closure=file.reviewKind==="closure";
  const [review,setReview]=useState<DiagnosticFile|null>(null),[reason,setReason]=useState(""),[ack,setAck]=useState(false),[pending,setPending]=useState(false),[message,setMessage]=useState("");
  const lock=useRef(false),dialog=useRef<HTMLDialogElement>(null);
  const valid=!!review?.reviewDigest&&review.reviewDigest===file.reviewDigest&&review.reviewKind===file.reviewKind&&!file.reviewed;
  const text=reason.trim(),validReason=!!text&&!text.includes("\0")&&new TextEncoder().encode(text).length<=8000;
  const blocked=busy||pending;
  useEffect(()=>{if(!review||!dialog.current)return;const el=dialog.current,opener=document.activeElement;el.showModal();return()=>{el.close();if(opener instanceof HTMLElement&&opener.isConnected)opener.focus();};},[review]);
  async function confirm(){
    if(blocked||lock.current||!review||!valid||!validReason||!ack)return;
    lock.current=true;setPending(true);setMessage("");
    try{if(await send({type:closure?"reviewStartupClosure":"reviewStartupIntent",relativePath:review.relativePath,evidenceDigest:review.reviewDigest!,reason:text})){setReview(null);setMessage("复核已保存，原文件保留；重新启动后仍须通过全部关联校验。");}else setMessage("复核未确认完成，请返回刷新诊断后核对。不会自动重发。");}
    catch{setMessage("连接中断，请返回刷新诊断后核对。");}
    finally{lock.current=false;setPending(false);}
  }
  return <>
    {file.reviewed?<p>{closure?"已复核并保留原文；重启后请重新审阅关闭操作。":"已复核并保留原文。尝试编号禁止复用；仍需重新启动并校验。"}</p>:file.reviewDigest?<button className="secondary-button" disabled={blocked} onClick={()=>{setReview(file);setReason("");setAck(false);setMessage("");}}>{closure?"复核半写关闭记录…":"复核半写启动意图…"}</button>:<p>此记录不能直接复核，请保留原证据继续核查。</p>}
    {message&&!review&&<p role="status">{message}</p>}
    <dialog ref={dialog} className="panel modal unsent-assignment-dialog" aria-labelledby="startup-intent-review-title" onCancel={e=>{e.preventDefault();if(!blocked)setReview(null);}}>{review&&<>
      <h2 id="startup-intent-review-title">{closure?"确认复核半写关闭记录":"确认复核半写启动意图"}</h2>
      <p style={{overflowWrap:"anywhere"}}>{review.relativePath}</p>
      <p>{closure?"将保留关闭残片与有效启动身份，另存本次复核说明。重新启动后仍须使用原关闭入口再次审阅，复核本身不会关闭启动或改变任务结果。已有分配或执行证据冲突时仍会停在诊断模式。":"将保留原文件、另存本次说明，并永久禁止复用此尝试编号。无法还原的启动内容不会被补造，任务不会被标为完成或取消。若发现已有分配或执行证据，重启后仍会停在诊断模式。"}</p>
      <label htmlFor="startup-intent-review-reason">本次复核说明</label><textarea id="startup-intent-review-reason" value={reason} onChange={e=>setReason(e.target.value)} disabled={blocked} rows={3}/>
      <label className="checkbox-label"><input type="checkbox" checked={ack} onChange={e=>setAck(e.target.checked)} disabled={blocked}/>{closure?"我已核对目标，并理解仍须重新审阅关闭操作":"我已核对目标，并理解旧尝试编号不能再次使用"}</label>
      {!valid&&<p role="alert">记录已变化，请返回刷新诊断。</p>}{message&&<p role="alert">{message}</p>}
      <div className="modal-actions"><button className="secondary-button" disabled={blocked} onClick={()=>setReview(null)}>返回检查</button><button className="primary" disabled={blocked||!valid||!validReason||!ack} onClick={()=>void confirm()}>{closure?"保存关闭复核":"保存启动复核"}</button></div>
    </>}</dialog>
  </>;
}
