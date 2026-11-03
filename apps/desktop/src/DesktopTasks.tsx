import { ErrorText } from "./ErrorText";
import {useRef,useState} from 'react';
import type {Command,DesktopTaskReview,DesktopTaskStatus,Snapshot} from './contracts';

export function DesktopTasks({data,busy,send}:{data:Snapshot|null;busy:boolean;send:(c:Command)=>Promise<boolean>}) {
  const access=data?.desktopAccess;
  const prepared=access?.taskReview?.observation.window.app==='qoder'?access.taskReview:undefined;
  const chat=(r:DesktopTaskReview)=>r.observation.sessionId??`${access?.bindings.find(b=>b.id===r.bindingId)?.label||r.observation.window.title||'WorkBuddy'}（本地关联）`;
  const [review,setReview]=useState<DesktopTaskReview|null>(null);
  const [ack,setAck]=useState(false);
  const [retire,setRetire]=useState<DesktopTaskStatus|null>(null);
  const [reason,setReason]=useState('');
  const [retireAck,setRetireAck]=useState(false);
  const [stop,setStop]=useState<DesktopTaskStatus|null>(null);
  const [stopAck,setStopAck]=useState(false);
  const [pending,setPending]=useState(false);
  const inFlight=useRef(false);
  const blocked=busy||pending||!!access?.checking;
  const jobs=access?.tasks??[];
  const unresolved=jobs.some(j=>!j.submitted&&!j.retired&&!j.cancellation?.confirmed)||!!access?.trials.some(t=>!t.proof);
  const stopCurrent=stop&&jobs.find(j=>j.review.id===stop.review.id);
  const stopBlocked=busy||pending||!stopCurrent||stopCurrent.stopping||!!stopCurrent.reviewedStopRecord||!!stopCurrent.cancellation||stopCurrent.submitted||stopCurrent.retired||!!access?.checking&&!stopCurrent.active;
  const assigned=(data?.workflows??[]).flatMap(w=>Object.entries(w.state.tasks).flatMap(([id,t])=>{
    if(!t)return [];
    const spec=w.revision?.execution.find(e=>e.taskId===id);
    return t.state==='assigned'&&t.attempt&&spec?.desktopWorker?.binding.observation.window.app==='qoder'&&!jobs.some(j=>j.review.projectId===w.project.id&&j.review.attempt.id===t.attempt)
      ? [{project:w.project.id,task:id,attempt:t.attempt,goal:w.revision?.workflow.tasks.find(t=>t.id===id)?.goal??id}] : [];
  }));
  const runPaused=(project:string)=>data?.projectRuns.some(r=>r.projectId===project&&r.revisionDigest===data.workflows.find(w=>w.project.id===project)?.revisionDigest&&r.state!=="active"&&r.state!=="completed"&&r.state!=="superseded")??false;
  const current=!!review&&review.id===prepared?.id&&Date.now()-Date.parse(review.observation.capturedAt)<=120000;
  async function act(command:Command) {
    if(inFlight.current)return false;
    inFlight.current=true;setPending(true);
    try{return await send(command);}finally{inFlight.current=false;setPending(false);}
  }
  if(!assigned.length&&!jobs.length&&!prepared&&!review)return null;
  return <section className="workflow-plans desktop-tasks" aria-label="桌面编程任务">
    <h2>桌面编程任务</h2>
    <p className="secondary">仅处理已批准且由副指挥分配的尝试。发送后由后台核对回复、封存文件并提交；完成仍需独立验收和主指挥收口。</p>
    {access?.error&&<p role="alert"><ErrorText error={access.error}/></p>}
    {assigned.map(t=><article className="panel" key={`${t.project}:${t.attempt}`}><h3>{t.goal}</h3><p>{t.project} · {t.task} · 已分配，尚未发送</p>
      {runPaused(t.project)&&<p>项目调度已暂停或需要处理，请先核对并恢复运行授权。</p>}
      <button className="secondary-button" disabled={blocked||unresolved||runPaused(t.project)} onClick={()=>void act({type:'prepareDesktopTask',projectId:t.project,taskId:t.task,attemptId:t.attempt})}>准备任务消息</button>
    </article>)}
    {prepared&&<article className="panel"><h3>任务消息已准备</h3><p>先核对完整任务、项目目录、模型和目标聊天，再确认发送。</p><button className="primary" disabled={blocked} onClick={()=>{setReview(prepared);setAck(false);}}>审阅编程任务</button></article>}
    {jobs.map(job=>{
      const r=job.review,w=data?.workflows.find(w=>w.project.id===r.projectId),task=w?.state.tasks[r.attempt.taskId];
      const retiredAdapter=r.observation.window.app==='workbuddy';
      const accepted=task?.attempt===r.attempt.id&&task.state==='accepted'&&task.verified;
      return <article className="panel" key={r.id} data-desktop-attempt={r.attempt.id}>
        <h3>{job.retired?'本地跟踪已结束，结果未确认':job.cancellation?.confirmed?'已确认取消，修改保留':accepted?'已独立验收并收口':job.active?(job.stopping?'正在核对停止结果':job.reading?'正在回读原任务':'正在执行桌面任务'):job.submitted?'交付已提交，等待验收':'任务结果待核对'}</h3>
        <p>{r.projectId} · {r.attempt.taskId} · {r.observation.modelLabel}</p><p>聊天：{chat(r)}</p>
        {retiredAdapter&&<p>WorkBuddy 支持已移除；原记录保留，不再连接外部应用。未决结果可在核对后结束本地跟踪。</p>}
        {!job.retired&&job.error&&<p role="alert"><ErrorText error={job.error}/></p>}
        {job.active&&<p>可继续查看工作台。外部应用可能使用自己的工具或等待批准，请保持目标聊天不变。</p>}
        {job.progress?.latest&&<p data-desktop-progress={r.attempt.id}>最近界面观察：{job.progress.latest.statusText||(job.progress.latest.observation.busy?'忙碌或等待批准':'空闲')} · {new Date(job.progress.latest.observation.capturedAt).toLocaleTimeString('zh-CN')}。{job.progress.error?'后续观察暂不可用；这是上次结果。':'仅反映原聊天界面，完成仍以独立验收为准。'}</p>}
        {job.progress?.error&&<p className="secondary">进度观察：{job.progress.error}。原任务不会因此重发或被判定失败。</p>}
        {job.reviewedStopRecord&&!job.retired&&<p>原停止记录已复核，原文保留；结果仍未确认。当前仅恢复本地管理，不能继续回读、发送或停止原尝试。核对外部应用后可结束本地跟踪。</p>}
        {job.cancellation&&<p>已记录停止请求：{job.cancellation.reason}。{job.cancellation.confirmed?'已核实原任务取消，已有修改保留，后续需要修订方案。':accepted?'核对时原任务已完成，随后已通过独立验收并由主指挥收口。':job.submitted?'核对时原任务已完成，仍需独立验收和主指挥收口。':'后续调度已暂停；停止结果待核对，不会再次发送停止动作。'}</p>}
        {!retiredAdapter&&!job.submitted&&!job.retired&&!job.cancellation&&!job.stopping&&!job.reviewedStopRecord&&<button className="secondary-button" disabled={busy||pending||!!access?.checking&&!job.active} onClick={()=>{setStop(job);setReason('');setStopAck(false);}}>停止原桌面任务</button>}
        {job.retired&&<p>原消息、回复记录和文件修改保留；这不代表外部应用已停止或任务成功。后续需要修订并重新批准方案、重新核对聊天关联。</p>}
        {!job.submitted&&!job.active&&!job.retired&&!job.cancellation?.confirmed&&<>{!retiredAdapter&&!job.reviewedStopRecord&&<p>原意图已保留，可能已经发送或留下草稿。请核对原聊天；回读不会输入文字或重新发送。</p>}<button className="secondary-button" disabled={blocked||retiredAdapter||!!job.reviewedStopRecord} onClick={()=>void act({type:'readDesktopTask',projectId:r.projectId,attemptId:r.attempt.id,readerSha256:access?.bridgeSha256??null})}>回读原任务结果</button>{job.retirementDigest&&<button className="secondary-button" disabled={blocked} onClick={()=>{setRetire(job);setReason('');setRetireAck(false);}}>结束本地跟踪</button>}</>}
        {!retiredAdapter&&!job.submitted&&!job.retired&&access?.bridgeSha256!==r.bridgeSha256&&<p>适配器已更新；回读会使用当前版本只读核对，原发送意图保留。</p>}
        <details><summary>原任务消息与记录</summary><p>目录：{r.attempt.binding.workspace}</p><p>尝试：{r.attempt.id}</p><p>读取时间：{new Date(r.observation.capturedAt).toLocaleString('zh-CN')}</p><p style={{whiteSpace:'pre-wrap',overflowWrap:'anywhere'}}>{r.prompt}</p></details>
      </article>;
    })}
    {stop&&<div className="modal-backdrop"><section className="modal panel execution-review" role="dialog" aria-modal="true" aria-labelledby="desktop-stop-title">
      <h2 id="desktop-stop-title">停止原桌面任务</h2>
      <p>{stop.review.projectId} · {stop.review.attempt.taskId}</p><p>聊天：{chat(stop.review)}</p><p>模型：{stop.review.observation.modelLabel}</p>
      <p>先暂停后续调度，再核对并停止这一条原任务。只有核实原任务终态才会显示取消成功；已有文件修改会保留。如果核对时任务已完成，仍需独立验收。</p>
      <label>停止原因<textarea value={reason} maxLength={2000} disabled={busy||pending} onChange={e=>setReason(e.target.value)}/></label>
      <label className="checkbox-label"><input type="checkbox" checked={stopAck} disabled={busy||pending} onChange={e=>setStopAck(e.target.checked)}/>我确认停止上述原任务，并保留已有修改</label>
      <div className="card-footer"><button className="secondary-button" disabled={busy||pending} onClick={()=>setStop(null)}>返回核对</button><button className="primary" disabled={stopBlocked||!stopAck||!reason.trim()} onClick={async()=>{if(await act({type:'cancelDesktopTask',projectId:stop.review.projectId,attemptId:stop.review.attempt.id,reviewDigest:stop.review.id,reason}))setStop(null);}}>确认停止原任务一次</button></div>
    </section></div>}
    {retire&&<div className="modal-backdrop"><section className="modal panel execution-review" role="dialog" aria-modal="true" aria-labelledby="desktop-retire-title">
      <h2 id="desktop-retire-title">结束未知桌面任务的本地跟踪</h2>
      <p>{retire.review.projectId} · {retire.review.attempt.taskId}</p><p>聊天：{chat(retire.review)}</p><p>尝试：{retire.review.attempt.id}</p>
      <p>原执行结果和费用仍未确认，已有文件修改和原消息记录会保留。此操作不会停止外部应用，也不会重发消息或标记验收通过。后续须修订并重新批准方案、重新核对聊天关联。</p>
      <label>处置原因<textarea value={reason} maxLength={2000} disabled={blocked} onChange={e=>setReason(e.target.value)}/></label>
      <label className="checkbox-label"><input type="checkbox" checked={retireAck} disabled={blocked} onChange={e=>setRetireAck(e.target.checked)}/>我已自行处理外部应用的运行状态，确认只结束本地跟踪，原结果仍未核实</label>
      <div className="card-footer"><button className="secondary-button" disabled={blocked} onClick={()=>setRetire(null)}>返回核对</button><button className="primary" disabled={blocked||!retireAck||!reason.trim()||!retire.retirementDigest||jobs.find(j=>j.review.id===retire.review.id)?.retirementDigest!==retire.retirementDigest} onClick={async()=>{if(retire.retirementDigest&&await act({type:'retireUnknownDesktop',projectId:retire.review.projectId,attemptId:retire.review.attempt.id,evidenceDigest:retire.retirementDigest,reason}))setRetire(null);}}>确认结束本地跟踪</button></div>
    </section></div>}
    {review&&<div className="modal-backdrop"><section className="modal panel execution-review" role="dialog" aria-modal="true" aria-labelledby="desktop-task-title">
      <h2 id="desktop-task-title">确认发送编程任务</h2>
      <p>{review.observation.window.title} · {review.observation.window.app} {review.observation.window.version}</p>
      <p>模型：{review.observation.modelLabel}</p><p>聊天：{chat(review)}</p><p>项目目录：{review.attempt.binding.workspace}</p>
      <p>会发送下面这一条任务消息，可能产生模型费用。工具权限由外部应用管理；请核对该应用当前目录与上述目录一致。</p>
      {review.continuation&&<p>已核对上一任务及完整回复，继续使用原聊天。上一尝试：{review.continuation.previousAttemptId}</p>}
      <blockquote style={{whiteSpace:'pre-wrap',overflowWrap:'anywhere'}}>{review.prompt}</blockquote>
      <label className="checkbox-label"><input type="checkbox" checked={ack} disabled={blocked} onChange={e=>setAck(e.target.checked)}/>我已核对目标聊天、项目目录和完整任务，确认发送一次</label>
      {!current&&<p role="alert">准备内容已变化或过期，请返回重新准备。</p>}
      {runPaused(review.projectId)&&<p role="alert">运行授权已暂停或需要处理，当前不能发送。</p>}
      <div className="card-footer"><button className="secondary-button" disabled={blocked} onClick={()=>setReview(null)}>返回检查</button><button className="primary" disabled={blocked||!current||!ack||runPaused(review.projectId)} onClick={async()=>{if(await act({type:'sendDesktopTask',reviewDigest:review.id}))setReview(null);}}>确认发送任务一次</button></div>
    </section></div>}
  </section>;
}
