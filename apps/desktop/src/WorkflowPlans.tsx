import { ExecutionAuthorization } from "./ExecutionAuthorization";
import { WorkerAccountAuthorization } from "./WorkerAccountAuthorization";
import { useEffect, useRef, useState } from "react";
import type { Command, ExecutionSnapshot, ProjectRevisionReview, SavedProjectSetup, VerificationDetail, VerificationJob, VerificationState, VerificationStageState } from "./contracts";
type Send = (command: Command) => Promise<boolean>;
const projectName = (path: string) => path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
const verificationLabels: Record<VerificationState,string> = {running:"验收进行中",cancelling:"正在停止验收",succeeded:"验收检查通过",failed:"验收未通过",unknown:"验收待对账",abandoned:"本次验收已放弃"};
const stageLabels:Record<VerificationStageState,string>={passed:"检查通过",failed:"检查失败",timedOut:"检查超时",observed:"已有观察，待整体验收确认",pending:"尚无完整观察",notRun:"未运行（前项超时）",unknown:"结果待核对"};
export function VerificationJobs({jobs,detail,send,busy}:{jobs:VerificationJob[];detail:VerificationDetail|null;send:Send;busy:boolean}) {
  const [review,setReview]=useState<VerificationJob|null>(null);
  const [selected,setSelected]=useState<VerificationJob|null>(null);
  const [view,setView]=useState<VerificationDetail|null>(null);
  const [readFailed,setReadFailed]=useState(false);
  const [recoverReview,setRecoverReview]=useState(false);
  useEffect(()=>{if(detail && selected?.projectId===detail.projectId && selected.operationId===detail.operationId)setView(detail);},[detail]);
  async function read(job:VerificationJob) {
    setSelected(job);setView(null);setReadFailed(false);setRecoverReview(false);
    if(!await send({type:"readVerificationDetail",projectId:job.projectId,operationId:job.operationId}))setReadFailed(true);
  }
  const [reason,setReason]=useState("");
  if (!jobs.length) return null;
  return <section className="workflow-plans" aria-label="独立验收记录"><h2>独立验收</h2>{jobs.map(job => <article className="panel" key={`${job.projectId}:${job.operationId}`}>
    <div className="section-heading"><h3>任务 {job.taskId}</h3><span className="badge">{verificationLabels[job.state]}</span></div>
    <p className="secondary">项目 {job.projectId} · {new Date(job.updatedAt).toLocaleString("zh-CN")}</p>
    <button className="secondary-button" disabled={busy} onClick={()=>void read(job)}>查看阶段结果</button>
    {job.message && <p>{job.message.includes("cancelled") ? "验收已停止，任务未标记为通过。" : job.message}</p>}
    {job.state === "unknown" && <p>这次验收没有可确认的完整结果，系统不会自动重新执行。</p>}
    {job.state === "unknown" && <button className="secondary-button" disabled={busy || !job.reconciliationDigest} onClick={()=>{setReason("");setReview(job);}}>处理待对账验收</button>}
    {job.state === "running" && <button className="secondary-button" disabled={busy} onClick={()=>void send({type:"cancelVerification",projectId:job.projectId,operationId:job.operationId})}>停止验收</button>}
  </article>)}{review && <div className="modal-backdrop"><section role="dialog" aria-modal="true" aria-labelledby="reconciliation-title" className="modal panel execution-review">
    <h2 id="reconciliation-title">放弃本次验收</h2>
    <p>任务 {review.taskId} · 验收 {review.operationId}</p>
    <p>保留本次日志和产物副本，将本次验收记录标记为已放弃。任务不会因此通过，也不会自动重跑检查。</p>
    <p className="secondary">后续如需重新检查，须由调度另行启动。当前证据变化后需要重新审阅。</p>
    <label htmlFor="abandon-reason">放弃原因</label><textarea id="abandon-reason" value={reason} maxLength={500} onChange={event=>setReason(event.target.value)} />
    <div className="card-footer"><button className="secondary-button" disabled={busy} onClick={()=>setReview(null)}>返回检查</button><button className="primary" disabled={busy || !reason.trim() || !review.reconciliationDigest} onClick={async()=>{if(review.reconciliationDigest && await send({type:"abandonVerification",projectId:review.projectId,operationId:review.operationId,digest:review.reconciliationDigest,reason:reason.trim()}))setReview(null);}}>确认放弃本次验收</button></div>
  </section></div>}
  {selected&&<div className="modal-backdrop"><section className="modal panel execution-review" role="dialog" aria-modal="true" aria-labelledby="verification-detail-title">
    <h2 id="verification-detail-title">验收阶段结果</h2>
    <p>任务 {selected.taskId} · 验收 {selected.operationId}</p>
    {!view?<p>{readFailed?"详情暂不可读，请查看错误提示并重新读取。":"正在读取原验收证据…"}</p>:<>
      <p><strong>{verificationLabels[view.state]}</strong> · {view.ledgerRecorded?"已有关联的验收账本记录":"尚无本次验收账本终态"}</p>
      <p className="secondary">读取于 {new Date(view.observedAt).toLocaleString("zh-CN")}。各项观察不等于整体验收通过，读取不会启动或重跑检查。</p>
      {view.message&&<p>{view.message}</p>}
      {view.recoveryError&&<p className="secondary">暂不能补记：{view.recoveryError}</p>}
      {view.recovery&&!recoverReview&&<button className="secondary-button" disabled={busy} onClick={()=>setRecoverReview(true)}>审阅补记结果</button>}
      {view.recovery&&recoverReview&&<section className="panel" aria-label="补记验收结果确认">
        <h3>补记原验收结果</h3><p>已核对原方案、完整报告、阶段日志、工具与当前受测文件。结果为：{view.recovery.passed?"全部检查通过":"验收未通过"}。</p>
        <p>确认只将已有结果补入原验收记录，不会重新运行检查。项目后续调度将暂停，任务仍需主指挥收口或进一步处置。</p>
        <div className="card-footer"><button className="secondary-button" disabled={busy} onClick={()=>setRecoverReview(false)}>返回阶段结果</button><button className="primary" disabled={busy} onClick={async()=>{if(view.recovery&&await send({type:"recoverVerification",projectId:view.projectId,operationId:view.operationId,evidenceDigest:view.recovery.evidenceDigest}))await read(selected);}}>确认补记原结果</button></div>
      </section>}
      <ol>{view.stages.map(stage=><li key={stage.id}><h3>{stage.name}</h3><p>{stageLabels[stage.state]} · 时限 {stage.timeoutSeconds} 秒 · 退出码 {stage.exitCode??"无完整记录"}</p>
        <details><summary>命令摘要与日志引用</summary><p>命令 SHA-256：<code>{stage.commandDigest}</code></p>
          {[stage.stdout,stage.stderr].map((log,index)=>log?<div key={index}><strong>{index===0?"标准输出":"标准错误"}</strong><p><code>{log.path}</code></p><p>{log.bytes} 字节 · SHA-256 <code>{log.sha256}</code></p></div>:<p key={index}>{index===0?"标准输出":"标准错误"}尚无可确认引用</p>)}
        </details></li>)}</ol>
      <details><summary>原方案、产物与受测副本证据</summary><dl><dt>尝试</dt><dd>{view.attemptId}</dd><dt>原方案摘要</dt><dd><code>{view.revisionDigest}</code></dd><dt>产物清单摘要</dt><dd><code>{view.manifestDigest}</code></dd><dt>受测副本摘要</dt><dd><code>{view.snapshotDigest??"尚无完整副本记录"}</code></dd><dt>本次读取证据摘要</dt><dd><code>{view.evidenceDigest}</code></dd></dl>
        <ul>{view.artifacts.map(a=><li key={a.path}>{a.path} · {a.bytes} 字节<br/><code>{a.sha256}</code></li>)}</ul>
      </details>
    </>}
    <div className="card-footer"><button className="secondary-button" disabled={busy} onClick={()=>void read(selected)}>重新读取</button><button className="primary" onClick={()=>{setSelected(null);setView(null);}}>关闭详情</button></div>
  </section></div>}</section>;
}

export function ProjectRegistration({ send, busy, connected }: { send: Send; busy: boolean; connected: boolean }) {
  const [root, setRoot] = useState("D:\\test\\coding");
  const [timezone, setTimezone] = useState("Asia/Shanghai");
  return <section className="panel">
    <h2>关联编程项目</h2>
    <p className="secondary">选择已有项目目录，后续执行方案将在这里审阅。关联目录不会运行命令或修改项目文件。</p>
    <form onSubmit={async (event) => {
      event.preventDefault();
      if (await send({ type: "registerProject", project: { id: `project-${crypto.randomUUID()}`, root: root.trim(), timezone: timezone.trim() } })) setRoot(root.trim());
    }}>
      <label htmlFor="project-root">项目目录</label>
      <input id="project-root" required value={root} onChange={e => setRoot(e.target.value)} aria-describedby="project-root-help" />
      <p id="project-root-help" className="secondary">初始目录为 D:\test\coding，可按项目需要修改。点击关联后才登记；修改这里的目录不会迁移或改写已关联项目。</p>
      <label htmlFor="project-timezone">交接文档时区</label>
      <input id="project-timezone" required value={timezone} onChange={e => setTimezone(e.target.value)} list="project-timezones" aria-describedby="project-timezone-help" />
      <datalist id="project-timezones">{["Asia/Shanghai", "Asia/Tokyo", "Asia/Kolkata", "Europe/London", "America/New_York", "UTC"].map(zone => <option key={zone} value={zone} />)}</datalist>
      <p id="project-timezone-help" className="secondary">按此时区的接收日期分卷。支持 IANA 名称，例如 America/New_York。关联后固定，已有项目不会随电脑时区改变。</p>
      <button className="primary" disabled={busy || !connected}>关联项目目录</button>
    </form>
  </section>;
}

export function WorkflowPlans({ workflows, revisions, setups=[], send, busy }: { workflows: ExecutionSnapshot[]; revisions:ProjectRevisionReview[]; setups?:SavedProjectSetup[]; send: Send; busy: boolean }) {
  // Keep the exact displayed revision while the dialog is open. A newer server
  // proposal must reject this digest rather than silently approve new content.
  const [review, setReview] = useState<ExecutionSnapshot | null>(null);
  const [accountScope,setAccountScope]=useState<string|null>(null);
  const [submitted,setSubmitted]=useState(false);
  const submitLock=useRef(false);
  const setup=setups.find(s=>s.setup.projectId===review?.project.id&&s.setup.chiefDirectory);
  const scopeKey=setup&&review ? `${review.project.id}:${review.revisionDigest}:${setup.digest}` : null;
  const [revisionReview,setRevisionReview]=useState<ProjectRevisionReview|null>(null);
  const [revisionReason,setRevisionReason]=useState("");
  const [retireConfirmed,setRetireConfirmed]=useState(false);
  const revision = review?.revision;
  return <section className="workflow-plans">
    <h2>项目执行方案</h2>
    {workflows.length === 0 ? <div className="empty compact">在项目设置中关联目录后，这里会显示待审阅的执行方案。</div> : workflows.map(workflow =>
      <article className="panel plan-card" key={workflow.project.id}>
        <div className="section-heading"><h3>{projectName(workflow.project.root)}</h3>
          <span className={workflow.approval ? "badge approved" : "badge"}>{workflow.approval ? "执行方案已批准" : workflow.revision ? "待审阅" : "等待规划"}</span>
        </div>
        <p className="secondary">{workflow.project.root}</p>
        <p className="secondary">交接文档时区：{workflow.project.timezone}</p>
        {workflow.revision ? <>
          <p>方案版本 {workflow.revision.workflow.version} · {workflow.revision.workflow.tasks.length} 项任务</p>
          <ul>{workflow.revision.workflow.tasks.map(task => <li key={task.id}>{task.goal}</li>)}</ul>
          {workflow.state.revisionRequested&&<p>已请求修订下一版。请在上方更新需求配置并请求主指挥规划；原方案不会继续执行。</p>}
          {revisions.filter(r=>r.projectId===workflow.project.id).map(r=><button key={r.projectId} className="secondary-button" disabled={busy} onClick={()=>{setRevisionReason("");setRetireConfirmed(false);setRevisionReview(r);}}>{r.submittedTasks>0?"结束本版提交并修订":"修订下一版方案"}</button>)}
          {!workflow.approval && <button className="secondary-button" disabled={busy} onClick={() => {submitLock.current=false;setAccountScope(null);setSubmitted(false);setReview(workflow);}}>审阅执行方案</button>}
        </> : <p className="secondary">项目已关联。主指挥接入后将在这里提交方案，确认前不会派发任务。</p>}
      </article>
    )}
    {revisionReview&&<div className="modal-backdrop"><section className="modal panel execution-review" role="dialog" aria-modal="true" aria-labelledby="revision-request-title">
      <h2 id="revision-request-title">修订下一版方案</h2><p>当前方案已有 {revisionReview.acceptedTasks} 项收口、{revisionReview.cancelledTasks} 项取消、{revisionReview.retiredTasks??0} 项本地结束但远端未知、{revisionReview.pendingTasks} 项尚未开始。</p>
      {revisionReview.submittedTasks>0&&<section className="panel" aria-label="结束本版提交确认">
        <h3>结束本版提交并修订</h3><p>本版还有 {revisionReview.submittedTasks} 项已提交但未收口，其中 {revisionReview.verifiedSubmissions} 项检查通过、{revisionReview.submittedTasks-revisionReview.verifiedSubmissions} 项未通过或尚未完成检查。</p>
        <p>确认后，这些原提交将保留在历史中，不再沿旧方案验收或收口；已写入项目的文件不会回滚。若需再次执行，请在新方案中明确安排，并重新批准和启动。历史调用仍计入项目累计请求数。</p>
        <label className="submission-retirement-confirm"><input type="checkbox" checked={retireConfirmed} onChange={e=>setRetireConfirmed(e.target.checked)}/>我确认结束本版未收口的提交</label>
      </section>}
      {revisionReview.pendingTasks>0&&<p>确认后，旧方案中尚未开始的任务也将停止后续派发。新方案可以保留、调整或移除这些任务，需要重新审阅批准后才能执行。</p>}
      <p>保留原方案、尝试和验收记录，暂停旧运行并开放下一版规划。新方案须重新批准和明确启动，本次确认不会调用模型或执行任务。</p>
      <label htmlFor="revision-reason">修订原因</label><textarea id="revision-reason" value={revisionReason} onChange={e=>setRevisionReason(e.target.value)}/>
      <div className="card-footer"><button className="secondary-button" disabled={busy} onClick={()=>setRevisionReview(null)}>返回</button><button className="primary" disabled={busy||!revisionReason.trim()||(revisionReview.submittedTasks>0&&!retireConfirmed)} onClick={async()=>{if(await send({type:revisionReview.submittedTasks>0?"retireSubmittedPlan":"requestProjectRevision",projectId:revisionReview.projectId,evidenceDigest:revisionReview.evidenceDigest,reason:revisionReason}))setRevisionReview(null);}}>确认开放修订</button></div>
    </section></div>}
    {review && revision && <div className="modal-backdrop"><section className="modal panel execution-review" role="dialog" aria-modal="true" aria-labelledby="execution-review-title">
      <span className="section-tag">执行授权</span><h2 id="execution-review-title">审阅项目执行方案</h2>
      <ExecutionAuthorization revision={revision}/>
      <WorkerAccountAuthorization available={!!scopeKey} directories={setup?.setup.chiefDirectory?[setup.setup.chiefDirectory,...Object.values(setup.setup.workerDirectories??{}).flatMap(d=>d?[`${review.project.root}\\${d.relativePath.replaceAll('/','\\')}`]:[])]:[]} checked={!!scopeKey&&accountScope===scopeKey} disabled={busy||submitted} onChange={v=>setAccountScope(v?scopeKey:null)}/>
      {submitted&&<p role="status">原批准已提交，请读取原记录核对。不会自动重发批准或 UAC。</p>}
      <p className="secondary">授权仅对应以上方案；目录、模型、程序或权限变化后需要重新确认。</p>
      <div className="card-footer"><button className="secondary-button" disabled={busy} onClick={() => setReview(null)}>返回检查</button>
        <button className="primary" disabled={busy || submitted || !review.revisionDigest} onClick={async () => {
          if (!review.revisionDigest||submitted||submitLock.current) return;
          submitLock.current=true;
          setSubmitted(true);
          const command:Command=scopeKey&&accountScope===scopeKey&&setup ? {type:"approveRevisionWithWorkerAccounts",projectId:review.project.id,digest:review.revisionDigest,setupDigest:setup.digest} : {type:"approveRevision",projectId:review.project.id,digest:review.revisionDigest};
          try { if(await send(command))setReview(null); } catch { /* Unknown result stays frozen. */ }
        }}>确认执行授权</button></div>
    </section></div>}
  </section>;
}
