import { useEffect, useRef, useState } from "react";
import type { Command, OmpStopIssue, Snapshot } from "./contracts";
import "./CodexJobs.css";

export function OmpStopDiagnostics({data,send,busy,restart}:{data:Snapshot|null;send:(c:Command)=>Promise<boolean>;busy:boolean;restart:()=>Promise<boolean>}) {
  const issues=data?.ompStopIssues??[];
  const [review,setReview]=useState<OmpStopIssue|null>(null),[reason,setReason]=useState(""),[ack,setAck]=useState(false),[pending,setPending]=useState(false),[message,setMessage]=useState("");
  const lock=useRef(false),dialog=useRef<HTMLDialogElement>(null);
  const blocked=busy||pending;
  const current=review&&issues.find(i=>i.projectId===review.projectId&&i.attemptId===review.attemptId);
  const valid=!!review?.evidenceDigest&&current?.evidenceDigest===review.evidenceDigest&&!current.acknowledged;
  const text=reason.trim(),validReason=!!text&&!text.includes("\0")&&new TextEncoder().encode(text).length<=8000;
  useEffect(()=>{if(!review||!dialog.current)return;const el=dialog.current,opener=document.activeElement;el.showModal();return()=>{el.close();if(opener instanceof HTMLElement&&opener.isConnected)opener.focus();};},[review]);
  async function confirm(){
    if(!review||!valid||!validReason||!ack||blocked||lock.current)return;
    lock.current=true;setPending(true);setMessage("");
    try{
      const ok=await send({type:"acknowledgeOmpStopIssue",projectId:review.projectId,attemptId:review.attemptId,evidenceDigest:review.evidenceDigest!,reason:text});
      if(ok){setReview(null);setMessage("复核记录已保存，原文件保留。重启本地核心后重新核对任务；本次操作不会恢复调度。");}
      else {await send({type:"snapshot"});setMessage("复核未确认完成，请重新读取记录后检查。不会自动重发。");}
    }catch{setMessage("连接中断，请重新读取记录后核对。");}
    finally{lock.current=false;setPending(false);}
  }
  if(!issues.length)return null;
  return <section className="panel" aria-label="停止记录诊断">
    <h2>核心处于诊断模式</h2>
    <p>停止记录存在异常，调度和其他修改操作已冻结。可读取当前记录并复核未写完整的停止请求。</p>
    {message&&<p role="status">{message}</p>}
    {issues.every(i=>i.acknowledged)&&<button className="secondary-button" disabled={blocked} onClick={async()=>{if(lock.current)return;lock.current=true;setPending(true);try{await restart();}finally{lock.current=false;setPending(false);}}}>重启核心并重新核对</button>}
    {issues.map(i=><article key={i.projectId+":"+i.attemptId}>
      <h3>项目 {i.projectId} · 任务 {i.taskId} · 尝试 {i.attemptId}</h3>
      <p>{i.message}</p>
      <details><summary>查看原始证据位置与摘要</summary><p>{i.pendingPath}</p>{i.pendingSha256&&<><p>{i.pendingBytes} 字节</p><p style={{overflowWrap:"anywhere"}}>SHA-256：{i.pendingSha256}</p></>}</details>
      {i.acknowledged?<p>此记录已复核。重启本地核心后重新验证；原任务结果仍需核对。</p>:i.evidenceDigest?<button className="secondary-button" disabled={blocked} onClick={()=>{setReview(i);setReason("");setAck(false);setMessage("");}}>复核中断的停止记录…</button>:<p>此故障不能直接确认。请保留证据，按可信备份进行专项恢复。</p>}
    </article>)}
    <dialog ref={dialog} className="panel modal unsent-assignment-dialog" aria-labelledby="stop-review-title" onCancel={e=>{e.preventDefault();if(!blocked)setReview(null);}}>{review&&<>
      <h2 id="stop-review-title">确认复核中断停止记录</h2>
      <p>项目 {review.projectId} · 任务 {review.taskId} · 尝试 {review.attemptId}</p>
      <p>原停止记录未写完整，无法还原当时的停止原因或送达情况。将保留原文并另存本次复核说明，保持项目暂停。这不代表执行器已取消或任务已完成。</p>
      <label htmlFor="stop-review-reason">本次复核说明</label><textarea id="stop-review-reason" value={reason} onChange={e=>setReason(e.target.value)} disabled={blocked} rows={3}/>
      <label className="checkbox-label"><input type="checkbox" checked={ack} onChange={e=>setAck(e.target.checked)} disabled={blocked}/>我已核对目标，并理解任务结果仍未确认</label>
      {!valid&&<p role="alert">记录已变化，请返回重新读取。</p>}{message&&<p role="alert">{message}</p>}
      <div className="modal-actions"><button className="secondary-button" disabled={blocked} onClick={()=>setReview(null)}>返回检查</button><button className="primary" disabled={blocked||!valid||!validReason||!ack} onClick={()=>void confirm()}>保存复核记录</button></div>
    </>}</dialog>
  </section>;
}
