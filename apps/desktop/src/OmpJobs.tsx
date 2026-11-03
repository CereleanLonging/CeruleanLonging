import { ErrorText } from "./ErrorText";
import { useEffect, useRef, useState } from "react";
import type { Command, OmpJob, OmpJobState, Snapshot, UnsentAssignmentReview } from "./contracts";
import "./CodexJobs.css";
const labels:Record<OmpJobState,string>={starting:"正在连接执行器",executing:"编程执行中","cancel-requested":"停止请求已提交",submitted:"已提交，待验收",cancelled:"本次尝试已结束",unknown:"执行结果待核对",failed:"启动未完成",abandoned:"本次尝试已关闭"};
type Review={kind:"cancel"|"startup"|"unsent";job:OmpJob;unsent?:UnsentAssignmentReview};
const same=(a:{projectId:string;attemptId:string},b:{projectId:string;attemptId:string})=>a.projectId===b.projectId&&a.attemptId===b.attemptId;
export function OmpJobs({data,send,busy,openManagement}:{data:Snapshot|null;send:(c:Command)=>Promise<boolean>;busy:boolean;openManagement:(operation:string)=>void}){
  const [review,setReview]=useState<Review|null>(null),[reason,setReason]=useState(""),[ack,setAck]=useState(false),[pending,setPending]=useState(false),[message,setMessage]=useState("");
  const [lastCancellation,setLastCancellation]=useState<{projectId:string;attemptId:string}|null>(null);
  const lock=useRef(false),dialog=useRef<HTMLDialogElement>(null);
  const jobs=data?.ompJobs??[],blocked=busy||pending||!!data?.ompStopIssues?.length;
  const latest=review&&jobs.find(j=>same(j,review.job));
  const current=!!review&&!!latest&&(review.kind==="cancel"? ["starting","executing"].includes(latest.state):review.kind==="startup"?!!review.job.closureDigest&&latest.closureDigest===review.job.closureDigest:data?.unsentAssignments.some(u=>same(u,review.job)&&u.evidenceDigest===review.unsent?.evidenceDigest));
  const text=reason.trim(),valid=!!text&&!text.includes("\0")&&new TextEncoder().encode(text).length<=8000;
  useEffect(()=>{if(!review||!dialog.current)return;const element=dialog.current,opener=document.activeElement;element.showModal();return()=>{element.close();if(opener instanceof HTMLElement&&opener.isConnected)opener.focus();};},[review]);
  useEffect(()=>{
    const job=lastCancellation&&jobs.find(j=>same(j,lastCancellation));
    if(!job||["starting","executing","cancel-requested"].includes(job.state))return;
    setMessage(job.state==="submitted"?"本次任务已提交，仍需独立验收；后续调度保持暂停。":job.state==="unknown"?"停止结果尚未确认，请核对原始执行证据。后续调度保持暂停。":"本次尝试已结束，已有修改保留，后续调度保持暂停。");
    setLastCancellation(null);
  },[data,lastCancellation]);
  function open(value:Review){if(blocked)return;setReview(value);setReason("");setAck(false);setMessage("");}
  async function act(command:Command,notice:string){if(blocked||lock.current)return;lock.current=true;setPending(true);setMessage("");try{if(await send(command)){setReview(null);setMessage(notice);}else {await send({type:"snapshot"});setMessage("操作未确认完成，请核对最新记录。系统不会自动重发任务。");}}catch{setMessage("连接中断，请重新读取状态后核对结果。");}finally{lock.current=false;setPending(false);}}
  function confirm(){if(!review||!current||!valid||!ack)return;const {job}=review;
    const command:Command=review.kind==="cancel"?{type:"cancelOmp",projectId:job.projectId,attemptId:job.attemptId,reason:text}:review.kind==="startup"?{type:"abandonOmpStartup",projectId:job.projectId,attemptId:job.attemptId,evidenceDigest:job.closureDigest!,reason:text}:{type:"abandonUnsentAttempt",...review.unsent!,reason:text};
    if(review.kind==="cancel")setLastCancellation({projectId:job.projectId,attemptId:job.attemptId});
    void act(command,review.kind==="cancel"?"停止请求已提交，后续调度已暂停。请等待执行结果，已有修改保留。":"本次记录已关闭，原证据保留；没有重新发送任务。");
  }
  if(!jobs.length&&!review)return null;
  return <section className="workflow-plans" aria-label="OMP 执行记录"><h2>OMP 编程执行</h2>{message&&<p role="status">{message}</p>}
    {jobs.map(job=>{const w=data?.workflows.find(w=>w.project.id===job.projectId),task=w?.state.tasks[job.taskId],unsent=data?.unsentAssignments.find(u=>same(u,job));const live=["starting","executing"].includes(job.state),accepted=task?.attempt===job.attemptId&&task.state==="accepted";
      const goal=w?.revisionDigest===job.revisionDigest?w.revision?.workflow.tasks.find(t=>t.id===job.taskId)?.goal:null;
      return <article className="panel codex-job" data-omp-project={job.projectId} data-omp-attempt={job.attemptId} key={job.projectId+":"+job.attemptId}>
        <div className="section-heading"><h3>{goal??`任务 ${job.taskId}`}</h3><span className="badge">{accepted?"已独立验收并收口":unsent?"已分配但尚未派发":labels[job.state]}</span></div>
        <p>{job.worker.provider} · {job.worker.model}</p><details className="codex-record-identity"><summary>记录详情</summary><p>{w?.project.root??job.projectId}</p><p>任务：{job.taskId} · 尝试：{job.attemptId}</p>{job.worker.ompRuntimeParent&&<p>独立运行目录：{job.worker.ompRuntimeParent}</p>}<p>开始时间：{new Date(job.startedAt).toLocaleString("zh-CN")}</p>{job.managementOperation&&<p>分配记录：{job.managementOperation}</p>}</details>
        {job.message&&<p>{job.message}</p>}
        {job.state==="cancel-requested"&&<p>等待执行器确认，当前还不能认定已经停止。</p>}
        {job.state==="cancelled"&&<p>已有文件修改与记录保留，请核对任务状态后决定后续工作。</p>}
        {job.state==="submitted"&&!accepted&&<p>执行器已经提交产物，仍需独立验收及主指挥收口。</p>}
        <div className="card-footer">
          {live&&<button disabled={blocked} className="secondary-button" onClick={()=>open({kind:"cancel",job})}>停止当前 OMP 任务…</button>}
          {job.closureDigest&&<button disabled={blocked} className="secondary-button" onClick={()=>open({kind:"startup",job})}>关闭未分配启动…</button>}
          {unsent&&<button disabled={blocked} className="secondary-button" onClick={()=>open({kind:"unsent",job,unsent})}>放弃未派发尝试…</button>}
          {job.state==="unknown"&&!job.closureDigest&&!unsent&&task?.state==="unknown"&&task.attempt===job.attemptId&&<button disabled={blocked} className="secondary-button" onClick={()=>void act({type:"recoverOmp",projectId:job.projectId,attemptId:job.attemptId},"已核对原始证据，请查看最新任务状态。没有重新发送任务。")}>核对原始执行证据</button>}
          {job.managementOperation&&<button disabled={blocked} className="secondary-button" onClick={()=>openManagement(job.managementOperation!)}>查看分配决定</button>}
        </div>
        {job.state==="unknown"&&<p className="secondary">核对只读取原始执行证据；证据不足时保持待核对。文件改动可在下方“OMP 文件恢复”中审阅。</p>}
        {job.managementOperation&&["unknown","failed"].includes(job.state)&&!job.closureDigest&&!unsent&&task?.state==="pending"&&<p>分配准备可能尚未提交，请打开分配决定核对应用事务。</p>}
      </article>;
    })}
    <dialog ref={dialog} className="panel modal unsent-assignment-dialog" aria-labelledby="omp-control-title" onCancel={e=>{e.preventDefault();if(!blocked)setReview(null);}}>{review&&<>
      <h2 id="omp-control-title">{review.kind==="cancel"?"确认停止 OMP 任务":review.kind==="startup"?"确认关闭 OMP 启动":"确认放弃 OMP 未派发尝试"}</h2>
      <p>{data?.workflows.find(w=>w.project.id===review.job.projectId)?.project.root??review.job.projectId}</p><p>项目 {review.job.projectId} · 任务 {review.job.taskId}</p><p>尝试 {review.job.attemptId}</p>
      <p>{review.kind==="cancel"?"将暂停后续调度并请求停止当前执行；已有修改保留，停止结果需由执行器确认。":review.kind==="startup"?"关闭已证明尚未分配的启动，保留原始记录，不代表任务完成。":"本次分配尚无派发记录。放弃后任务回到待分配，不自动恢复调度。"}</p>
      <label htmlFor="omp-control-reason">原因</label><textarea id="omp-control-reason" value={reason} onChange={e=>setReason(e.target.value)} disabled={blocked} rows={3}/>
      <label className="checkbox-label"><input type="checkbox" checked={ack} onChange={e=>setAck(e.target.checked)} disabled={blocked}/>我已核对目标和处置影响</label>
      {!current&&<p role="alert">记录已变化，请返回刷新后重新审阅。</p>}{message&&<p role="alert"><ErrorText error={message}/></p>}
      <div className="modal-actions"><button className="secondary-button" disabled={blocked} onClick={()=>setReview(null)}>返回检查</button><button className="primary" disabled={blocked||!current||!valid||!ack} onClick={confirm}>确认处置</button></div>
    </>}</dialog>
  </section>;
}
