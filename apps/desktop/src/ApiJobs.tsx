import { ErrorText } from "./ErrorText";
import { useEffect, useRef, useState } from "react";
import type { Command, ApiJob, ApiJobState, Snapshot, UnsentAssignmentReview } from "./contracts";
import "./CodexJobs.css";
const labels:Record<ApiJobState,string>={starting:"正在准备 API 任务",executing:"编程执行中","cancel-requested":"停止请求已提交",submitted:"已提交，待验收",unknown:"结果尚未确认",abandoned:"已关闭本地尝试",retired:"本地已结束，远端结果未知"};
type Review={kind:"cancel"|"startup"|"unsent"|"retire";job:ApiJob;unsent?:UnsentAssignmentReview};
const same=(a:{projectId:string;attemptId:string},b:{projectId:string;attemptId:string})=>a.projectId===b.projectId&&a.attemptId===b.attemptId;
const service=(job:ApiJob)=>job.worker.target.kind==="openAi"?"OpenAI":({a6Api:"A6API 中转 · api.a6api.com",a6ApiVip:"A6API 中转 · api.a6api.vip",deepSeek:"DeepSeek",anthropic:"Claude",kimiChina:"Kimi 中国站",grok:"xAI Grok",qwenBeijing:"Qwen 北京",qwenSingapore:"Qwen 新加坡",qwenHongKong:"Qwen 中国香港"}[job.worker.target.endpoint.kind]);
export function ApiJobs({data,send,busy,openManagement}:{data:Snapshot|null;send:(c:Command)=>Promise<boolean>;busy:boolean;openManagement:(operation:string)=>void}){
  const [review,setReview]=useState<Review|null>(null),[reason,setReason]=useState(""),[ack,setAck]=useState(false),[pending,setPending]=useState(false),[message,setMessage]=useState("");
  const [lastCancellation,setLastCancellation]=useState<{projectId:string;attemptId:string}|null>(null);
  const lock=useRef(false),dialog=useRef<HTMLDialogElement>(null);
  const jobs=data?.apiJobs??[],blocked=busy||pending||!!data?.ompStopIssues?.length;
  const latest=review&&jobs.find(j=>same(j,review.job));
  const current=!!review&&!!latest&&(review.kind==="cancel"? ["starting","executing"].includes(latest.state):review.kind==="retire"?!!review.job.retirementDigest&&latest.retirementDigest===review.job.retirementDigest:review.kind==="startup"?!!review.job.closureDigest&&latest.closureDigest===review.job.closureDigest:data?.unsentAssignments.some(u=>same(u,review.job)&&u.evidenceDigest===review.unsent?.evidenceDigest));
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
    const command:Command=review.kind==="retire"?{type:"retireUnknownApi",projectId:job.projectId,attemptId:job.attemptId,evidenceDigest:job.retirementDigest!,reason:text}:review.kind==="cancel"?{type:"cancelApi",projectId:job.projectId,attemptId:job.attemptId,reason:text}:review.kind==="startup"?{type:"abandonApiStartup",projectId:job.projectId,attemptId:job.attemptId,evidenceDigest:job.closureDigest!,reason:text}:{type:"abandonUnsentAttempt",...review.unsent!,reason:text};
    if(review.kind==="cancel")setLastCancellation({projectId:job.projectId,attemptId:job.attemptId});
    void act(command,review.kind==="retire"?"本地尝试已结束，远端结果与费用仍未知。已有修改保留，请申请修订并重新批准方案。":review.kind==="cancel"?"停止请求已提交，后续调度已暂停。请等待后台结束，已有修改保留；不代表供应商确认取消。":"本次记录已关闭，原证据保留；没有重新发送任务。");
  }
  if(!jobs.length&&!review)return null;
  return <section className="workflow-plans" aria-label="API 执行记录"><h2>API 编程执行</h2>{message&&<p role="status">{message}</p>}
    {jobs.map(job=>{const w=data?.workflows.find(w=>w.project.id===job.projectId),task=w?.state.tasks[job.taskId],unsent=data?.unsentAssignments.find(u=>same(u,job));const live=["starting","executing"].includes(job.state),accepted=task?.attempt===job.attemptId&&task.state==="accepted";
      const goal=w?.revisionDigest===job.revisionDigest?w.revision?.workflow.tasks.find(t=>t.id===job.taskId)?.goal:null;
      return <article className="panel codex-job" data-api-project={job.projectId} data-api-attempt={job.attemptId} key={job.projectId+":"+job.attemptId}>
        <div className="section-heading"><h3>{goal??`任务 ${job.taskId}`}</h3><span className="badge">{accepted?"已独立验收并收口":unsent?"已分配但尚未派发":labels[job.state]}</span></div>
        <p>{service(job)} · {job.worker.model}</p><details className="codex-record-identity"><summary>记录详情</summary><p>{w?.project.root??job.projectId}</p><p>任务：{job.taskId} · 尝试：{job.attemptId}</p><p>开始时间：{new Date(job.startedAt).toLocaleString("zh-CN")}</p>{job.managementOperation&&<p>分配记录：{job.managementOperation}</p>}</details>
        {job.message&&<p>{job.message}</p>}
        {job.state==="cancel-requested"&&<p>等待本地后台结束；这不代表供应商已取消请求或停止计费。</p>}
        {job.state==="abandoned"&&<p>原始记录保留，任务未被标记完成。后续执行需要明确恢复调度。</p>}
        {job.state==="retired"&&<p>本地已结束，原请求结果及费用仍未确认；已有文件修改和历史预算保留。须申请修订并重新批准方案。</p>}
        {job.state==="submitted"&&!accepted&&<p>执行器已经提交产物，仍需独立验收及主指挥收口。</p>}
        <div className="card-footer">
          {live&&<button disabled={blocked} className="secondary-button" onClick={()=>open({kind:"cancel",job})}>停止当前 API 任务…</button>}
          {job.retirementDigest&&<button disabled={blocked} className="secondary-button" onClick={()=>open({kind:"retire",job})}>结束本地未知尝试…</button>}
          {job.closureDigest&&<button disabled={blocked} className="secondary-button" onClick={()=>open({kind:"startup",job})}>关闭未分配启动…</button>}
          {unsent&&<button disabled={blocked} className="secondary-button" onClick={()=>open({kind:"unsent",job,unsent})}>放弃未派发尝试…</button>}
          {job.state==="unknown"&&!job.closureDigest&&!unsent&&task?.state==="unknown"&&task.attempt===job.attemptId&&<button disabled={blocked} className="secondary-button" onClick={()=>void act({type:"recoverApi",projectId:job.projectId,attemptId:job.attemptId},"已核对原始证据，请查看最新任务状态。没有重新发送任务。")}>核对原始执行证据</button>}
          {job.managementOperation&&<button disabled={blocked} className="secondary-button" onClick={()=>openManagement(job.managementOperation!)}>查看分配决定</button>}
        </div>
        {job.state==="unknown"&&<p className="secondary">核对只读取原始执行证据；证据不足时保持待核对。已有文件修改保留；原证据不足时不会重新请求模型。</p>}
        {job.managementOperation&&["unknown"].includes(job.state)&&!job.closureDigest&&!unsent&&task?.state==="pending"&&<p>分配准备可能尚未提交，请打开分配决定核对应用事务。</p>}
      </article>;
    })}
    <dialog ref={dialog} className="panel modal unsent-assignment-dialog" aria-labelledby="api-control-title" onCancel={e=>{e.preventDefault();if(!blocked)setReview(null);}}>{review&&<>
      <h2 id="api-control-title">{review.kind==="retire"?"确认结束本地未知尝试":review.kind==="cancel"?"确认停止 API 任务":review.kind==="startup"?"确认关闭 API 启动":"确认放弃 API 未派发尝试"}</h2>
      <p>{data?.workflows.find(w=>w.project.id===review.job.projectId)?.project.root??review.job.projectId}</p><p>项目 {review.job.projectId} · 任务 {review.job.taskId}</p><p>尝试 {review.job.attemptId}</p>
      <p>{review.kind==="retire"?"本地后台已结束，但没有可靠完成记录。此操作不会取消供应商请求，也不代表执行成功或免计费。原请求、已有文件修改和预算占用保留；后续须申请修订并重新批准方案。":review.kind==="cancel"?"将暂停后续调度并请求停止本地 API 执行。已有修改保留；供应商可能已经处理请求并计费，结果仍需核对。":review.kind==="startup"?"关闭已证明尚未分配的启动，保留原始记录，不代表任务完成。":"本次分配尚无派发记录。放弃后任务回到待分配，不自动恢复调度。"}</p>
      <label htmlFor="api-control-reason">原因</label><textarea id="api-control-reason" value={reason} onChange={e=>setReason(e.target.value)} disabled={blocked} rows={3}/>
      <label className="checkbox-label"><input type="checkbox" checked={ack} onChange={e=>setAck(e.target.checked)} disabled={blocked}/>我已核对目标和处置影响</label>
      {!current&&<p role="alert">记录已变化，请返回刷新后重新审阅。</p>}{message&&<p role="alert"><ErrorText error={message}/></p>}
      <div className="modal-actions"><button className="secondary-button" disabled={blocked} onClick={()=>setReview(null)}>返回检查</button><button className="primary" disabled={blocked||!current||!valid||!ack} onClick={confirm}>确认处置</button></div>
    </>}</dialog>
  </section>;
}
