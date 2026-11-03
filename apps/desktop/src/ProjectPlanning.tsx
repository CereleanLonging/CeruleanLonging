import { WorkerAccountControls } from "./WorkerAccountControls";
import { MarkdownImport } from "./MarkdownImport";
import { AssertionFields, cleanAssertions } from "./AssertionFields";
import { ManagementSourceFields } from "./ManagementSourceFields";
import { WorkflowTemplateImport } from "./WorkflowTemplateImport";
import { useEffect, useState } from "react";
import { managementKeyMissing, managementModelList } from "./ManagementProviders";
import type { AcceptanceStageDraft, Command, ProjectSetupDraft, ProjectWorkerAdapter, Snapshot, SavedProjectSetup } from "./contracts";

import { ApiWorkerFields, apiWorkerOptions, newApiWorker } from "./ApiWorkerFields";
import { DesktopWorkerFields } from "./DesktopWorkerFields";
import { NativeWorkerCost } from "./NativeWorkerCost";
import { WorkerProfiles, restoreProfile } from "./WorkerProfiles";
import { effectiveSupervision, canRecordSupervision, supervisionConflictFor } from "./SupervisionFields";
import { ResourceFields, resourceNames, resourceError } from "./ResourceFields";
import { WorktreeFields } from "./WorktreeFields";
import { CurrencyBudgetFields, CurrencyBudgetReview } from "./CurrencyBudgetFields";

const empty:ProjectSetupDraft={workerAdapter:"codex",projectId:"",requirement:"",codexExecutable:"",workerModel:"",chiefModel:"",deputyModel:"",validatorId:"",checkArgs:[],artifactPaths:[],workerMaxRequests:3,managementMaxRequests:10,maxSeconds:180};
const states:Record<string,string>={awaitingExternal:"等待绑定的 Codex 聊天回填方案",running:"主指挥正在规划",ready:"回复待处理",applying:"方案正在入账",applied:"回复已处理",unknown:"请求结果待核对",discarded:"请求已放弃",abandoned:"应用事务已关闭",startupClosed:"启动记录已关闭"};
export function ProjectPlanning({data,busy,send}:{data:Snapshot|null;busy:boolean;send:(command:Command)=>Promise<boolean>}) {
  const [projectId,setProjectId]=useState("");
  const [draft,setDraft]=useState<ProjectSetupDraft>(empty);
  const [budgetError,setBudgetError]=useState<string>();
  const [draftMessage,setDraftMessage]=useState("");
  const draftKey=JSON.stringify(["cerulean-next-execution",data?.dataDirectory,projectId]);
  function keepNextDraft(){try{localStorage.setItem(draftKey,JSON.stringify({draft,artifacts,stages,basedOn:saved?.digest}));setDraftMessage("下一版草稿已保存在本机；当前任务保持原模型和目录。");}catch{setDraftMessage("无法保存本机草稿，请保留此窗口。");}}
  function loadNextDraft(){try{const x=JSON.parse(localStorage.getItem(draftKey)??"null");if(!x||x.draft?.projectId!==projectId||typeof x.artifacts!=="string"||!Array.isArray(x.stages))throw new Error();setDraft(x.draft);setArtifacts(x.artifacts);setStages(x.stages);setDirty(true);setDraftMessage("已载入下一版草稿，请重新审阅后保存配置。");}catch{setDraftMessage("没有可载入的下一版草稿。");}}
  const [planningBudgetReview,setPlanningBudgetReview]=useState<SavedProjectSetup>();
  const [artifacts,setArtifacts]=useState("");const [stages,setStages]=useState<AcceptanceStageDraft[]>([]);const [dirty,setDirty]=useState(false);
  const saved=data?.projectSetups?.find(s=>s.setup.projectId===projectId);
  useEffect(()=>{if(!projectId && data?.workflows.length)setProjectId(data.workflows[0].project.id);},[projectId,data?.workflows]);
  useEffect(()=>{
    const s=saved?.setup;
    setDraft(s ? {chiefDirectory:s.chiefDirectory,managementSources:s.managementSources,currencyBudget:s.policy.currencyBudget,resourceKeys:s.resourceKeys,profiles:s.profiles?.map(restoreProfile),worktree:s.worktree?{integration:s.worktree.integration?{taskId:s.worktree.integration.taskId,artifactPaths:s.worktree.integration.artifactPaths,validatorId:s.worktree.integration.validatorId,args:s.worktree.integration.check.args,timeoutSeconds:s.worktree.integration.check.timeoutSeconds}:undefined,inheritDependencies:s.worktree.input==="committed-raw-blobs-with-accepted-dependencies"?true:undefined,baseCommit:s.worktree.baseCommit,poolRoot:s.worktree.poolRoot,gitExecutable:s.worktree.git.path}:undefined,workerAdapter:s.workerAdapter??"codex",apiWorker:s.apiWorker,desktopWorker:s.desktopWorker,ompRuntimeParent:s.worker?.ompRuntimeParent,supervision:s.policy.supervision,projectId,requirement:s.requirement,codexExecutable:s.worker?.executable.path??"",workerModel:s.desktopWorker?.modelLabel??s.apiWorker?.model??s.worker?.model??"",chiefModel:s.chiefModel,deputyModel:s.deputyModel,validatorId:s.validatorId,checkArgs:s.check.args,artifactPaths:s.artifactPaths,workerMaxRequests:s.policy.maxRequests,managementMaxRequests:s.managementMaxRequests,maxSeconds:s.policy.maxSeconds} : {...empty,projectId,chiefDirectory:(data?.workflows.find(w=>w.project.id===projectId)?.project.root??"")+"-chief",codexExecutable:data?.codexConnection?.executable??"",validatorId:data?.validators[0]?.id??""});
    setArtifacts(s?.artifactPaths.join("\n")??"");
    setStages(s?.acceptance?.stages.map(stage=>({expectedAssertions:stage.check.expectedAssertions,id:stage.check.id,name:stage.check.name??stage.check.id,validatorId:stage.validatorId,args:stage.check.args,timeoutSeconds:stage.check.timeoutSeconds}))??[{expectedAssertions:s?.check.expectedAssertions,id:s?.check.id??"project-check",name:s?.check.name??"项目验收",validatorId:s?.validatorId??data?.validators[0]?.id??"",args:s?.check.args??[],timeoutSeconds:s?.check.timeoutSeconds??60}]);setDirty(false);
  },[projectId,saved?.digest]);
  function update<K extends keyof ProjectSetupDraft>(key:K,value:ProjectSetupDraft[K]) {setDraft(d=>({...d,[key]:value}));setDirty(true);}
  function changeStages(next:AcceptanceStageDraft[]) {setStages(next);setDirty(true);}
  function changeStage(index:number,patch:Partial<AcceptanceStageDraft>) {changeStages(stages.map((s,i)=>i===index?{...s,...patch}:s));}
  function moveStage(index:number,delta:number) {const next=[...stages];[next[index],next[index+delta]]=[next[index+delta],next[index]];changeStages(next);}
  const totalSeconds=stages.reduce((total,stage)=>total+stage.timeoutSeconds,0);
  const decisions=data?.managementDecisions?.filter(d=>d.projectId===projectId)??[];
  const statistics=data?.managementHistory.projects.find(project=>project.projectId===projectId);
  const requestCount=statistics?.total??0;
  const pending=(statistics?.pending??0)>0;
  const workflow=data?.workflows.find(w=>w.project.id===projectId);
  const desktopAdapter=draft.workerAdapter==="qoder"||draft.workerAdapter==="workbuddy";
  const retiredAdapter=draft.workerAdapter==="workbuddy"||(draft.profiles??[]).some(p=>p.workerAdapter==="workbuddy");
  const defaultAdapter = draft.workerAdapter ?? "codex";
  const defaultSupervisionConflict = supervisionConflictFor(defaultAdapter, draft.supervision);
  const supervisionConflict=defaultSupervisionConflict || (draft.profiles??[]).some(p=>supervisionConflictFor(p.workerAdapter,effectiveSupervision(p.supervisionOverride,draft.supervision)));
  const locked=!!workflow?.approval && !workflow.state.revisionRequested;
  return <section className="panel project-planning" aria-label="主指挥项目规划">
    <div className="section-heading"><h2>向主指挥提交需求</h2><span className="badge">{saved?`配置版本 ${saved.setup.version}`:"尚未配置"}</span></div>
    {!data?.workflows.length ? <p className="secondary">请先在“项目设置”关联项目目录，并登记独立验收工具。</p> : <>
      <label htmlFor="planning-project">规划项目</label><select id="planning-project" value={projectId} onChange={e=>setProjectId(e.target.value)}>{data.workflows.map(w=><option value={w.project.id} key={w.project.id}>{w.project.root}</option>)}</select>
      <p className="secondary">先保存需求与执行范围，再请求主指挥提出方案。方案生成后会出现在下方，审阅批准前不会派发编程任务。</p>
      <form onSubmit={async e=>{e.preventDefault();if(locked){keepNextDraft();return;}if(budgetError||retiredAdapter)return;if(resourceError(draft.resourceKeys)||(draft.profiles??[]).some(p=>resourceError(p.resourceKeys,draft.resourceKeys)))return;if(await send({type:"configureProject",draft:{...draft,resourceKeys:resourceNames(draft.resourceKeys).length?resourceNames(draft.resourceKeys):undefined,profiles:draft.profiles?.map(p=>({...p,acceptance:{...p.acceptance,stages:p.acceptance.stages.map(s=>({...s,expectedAssertions:cleanAssertions(s.expectedAssertions)}))},resourceKeys:resourceNames(p.resourceKeys).length?resourceNames(p.resourceKeys):undefined})),projectId,artifactPaths:artifacts.split(/\r?\n/).map(v=>v.trim()).filter(Boolean),validatorId:stages[0]?.validatorId??"",checkArgs:stages[0]?.args??[],acceptance:{schemaVersion:1,stages:stages.map(s=>({...s,expectedAssertions:cleanAssertions(s.expectedAssertions)}))}}})){setDirty(false);localStorage.removeItem(draftKey);setDraftMessage("");}}}>
        <fieldset disabled={busy||pending}>
          <WorkflowTemplateImport contextKey={JSON.stringify([projectId,saved?.digest,draft])} disabled={busy||pending} onApply={(requirement,supervision)=>{setDraft(d=>({...d,requirement,...(supervision?{supervision}:{})}));setDirty(true);}}/>
          <section className="panel" aria-label="角色与模型工作目录"><h3>角色与模型工作目录</h3>
            <label htmlFor="planning-chief-directory">主指挥独立目录</label><input id="planning-chief-directory" readOnly={!!saved?.setup.chiefDirectory} value={draft.chiefDirectory??""} onChange={e=>{update("chiefDirectory",e.target.value||undefined);if(e.target.value)update("worktree",undefined);}} placeholder={(workflow?.project.root??"")+"-chief"}/>
            <p>副指挥与执行端总目录：<code>{workflow?.project.root}</code></p>
            <p className="secondary">副指挥只读调取主指挥原需求与方案。执行模型首次接入时，在总目录的 executors 下建立独立子目录；最多五个，切回已有模型复用原目录。模型切换不能改变正在运行尝试的目录。</p>
            {!draft.chiefDirectory&&<p role="status">当前保持旧配置；填写独立目录后，保存新版配置并重新批准，启用目录隔离与完整交付审阅。</p>}
            {saved?.setup.workerDirectories&&<ul>{Object.values(saved.setup.workerDirectories).map(d=>d&&<li key={d.identity}><code>{d.relativePath}</code></li>)}</ul>}
          </section>
          <label htmlFor="planning-requirement">项目需求</label><textarea id="planning-requirement" required rows={4} value={draft.requirement} onChange={e=>update("requirement",e.target.value)} placeholder="描述要解决的问题、预期行为和完成标准。" />
          <MarkdownImport mode="requirement" contextKey={JSON.stringify([projectId,saved?.digest,draft.requirement])} disabled={busy||pending} onApply={text=>update("requirement",text)}/>
          <div className="planning-fields">
            <ManagementSourceFields draft={draft} onChange={next=>{setDraft(next);setDirty(true);}}/>
          </div>
          <section className="role-model-fields" aria-label="执行端模型配置"><h3>执行端</h3>
          <div className="planning-fields">
            <div><label htmlFor="planning-adapter">编程执行器</label><select id="planning-adapter" value={draft.workerAdapter??"codex"} onChange={e=>{setDraft(d=>({...d,workerAdapter:e.target.value as ProjectWorkerAdapter,worktree:["qoder","workbuddy"].includes(e.target.value)?undefined:d.worktree,apiWorker:newApiWorker(e.target.value as ProjectWorkerAdapter),desktopWorker:undefined,codexExecutable:"",workerModel:"",ompRuntimeParent:undefined}));setDirty(true);}}><option value="codex">Codex</option><option value="omp">OMP · DeepSeek</option><option value="qoder">Qoder · 已有桌面会话</option>{draft.workerAdapter==="workbuddy"&&<option value="workbuddy" disabled>WorkBuddy · 支持已移除</option>}{apiWorkerOptions.map(([id,label])=><option key={id} value={id}>{label}</option>)}</select></div>
            {!draft.apiWorker&&!desktopAdapter&&<div><label htmlFor="planning-executable">编程程序路径</label><input id="planning-executable" required value={draft.codexExecutable} onChange={e=>update("codexExecutable",e.target.value)} placeholder={draft.workerAdapter==="omp"?"OMP 可执行文件的绝对路径":"Codex 可执行文件的绝对路径"} /></div>}
            {!desktopAdapter&&<div><label htmlFor="planning-worker">编程模型 ID</label><input id="planning-worker" required list="planning-worker-models" value={draft.workerModel} onChange={e=>{const model=e.target.value;setDraft(d=>({...d,workerModel:model,apiWorker:d.apiWorker?{...d.apiWorker,model}:undefined}));setDirty(true);}} /></div>}
          </div>
          {draft.workerAdapter==="workbuddy"&&<p role="alert">WorkBuddy 支持已移除，历史配置保留。请选择其他执行器后保存新版配置。</p>}
          {draft.workerAdapter==="qoder"&&<DesktopWorkerFields isolationEnabled={!!draft.chiefDirectory} data={data} projectId={projectId} app="qoder" worker={draft.desktopWorker} onChange={worker=>{setDraft(d=>({...d,desktopWorker:worker,workerModel:worker?.modelLabel??""}));setDirty(true);}}/>}
          {draft.apiWorker&&<ApiWorkerFields worker={draft.apiWorker} onChange={api=>update("apiWorker",api)}/>}
          <NativeWorkerCost adapter={draft.workerAdapter??"codex"}/>
          {draft.workerAdapter==="omp"&&(draft.chiefDirectory?<p className="secondary">OMP 运行配置固定保存在该模型目录内，与其他执行模型隔离。</p>:<><label htmlFor="planning-omp-runtime">OMP 独立运行目录（可选）</label><input id="planning-omp-runtime" value={draft.ompRuntimeParent??""} onChange={e=>update("ompRuntimeParent",e.target.value||undefined)} placeholder="已存在、可写的专用目录绝对路径"/><p className="secondary">用户目录含其他 Agent 配置时，请选择独立位置。须与项目和工作台数据目录分开，祖先目录不能含隐含配置。留空沿用工作台数据目录；更改后需重新规划和批准，历史运行文件保留。</p></>)}
          <datalist id="planning-chief-models">{managementModelList(data,draft.managementSources?.chief).map(m=><option key={m} value={m}/>)}</datalist><datalist id="planning-deputy-models">{managementModelList(data,draft.managementSources?.deputy).map(m=><option key={m} value={m}/>)}</datalist><datalist id="planning-worker-models">{(draft.workerAdapter??"codex")==="codex"&&data.codexConnection?.models.map(m=><option key={m} value={m}/>)}</datalist>
          <WorktreeFields disabledReason={draft.chiefDirectory?"已启用固定模型目录，不与按尝试创建的 Git 目录池混用。":undefined} validators={data.validators} value={draft.worktree??undefined} disabled={!!draft.chiefDirectory||desktopAdapter||(draft.profiles??[]).some(p=>p.workerAdapter==="qoder"||p.workerAdapter==="workbuddy")} onChange={value=>update("worktree",value)}/>
          <ResourceFields id="planning-resources" value={draft.resourceKeys} onChange={value=>update("resourceKeys",value)}/>
          <WorkerProfiles isolationEnabled={!!draft.chiefDirectory} supervision={draft.supervision} commonResources={draft.resourceKeys??[]} data={data} projectId={projectId} value={draft.profiles??[]} worktree={!!draft.worktree} onChange={profiles=>{setDraft(d=>({...d,profiles:profiles.length?profiles:undefined}));setDirty(true);}}/>
          </section>
          <CurrencyBudgetFields resetKey={projectId+":"+(saved?.digest??"")} resetValue={saved?.setup.policy.currencyBudget} draft={draft} value={draft.currencyBudget} onChange={value=>update("currencyBudget",value)} onInvalid={setBudgetError}/>
          <h3>有序验收检查</h3>
          <p className="secondary">按以下顺序执行，全部通过后才可收口。每项使用已登记工具，参数按行传递。</p>
          {stages.map((stage,index)=><div className="panel" key={stage.id}>
            <div className="section-heading"><strong>第 {index+1} 项</strong></div>
            <label htmlFor={"stage-name-"+index}>检查名称</label><input id={"stage-name-"+index} required value={stage.name} onChange={e=>changeStage(index,{name:e.target.value})}/>
            <label htmlFor={index===0?"planning-validator":"stage-validator-"+index}>独立验收工具</label><select id={index===0?"planning-validator":"stage-validator-"+index} required value={stage.validatorId} onChange={e=>changeStage(index,{validatorId:e.target.value})}><option value="">选择已登记工具</option>{data.validators.map(v=><option key={v.id} value={v.id}>{v.name}</option>)}</select>
            <label htmlFor={index===0?"planning-args":"stage-args-"+index}>验收参数（每行一个参数）</label><textarea id={index===0?"planning-args":"stage-args-"+index} rows={3} value={stage.args.join("\n")} onChange={e=>changeStage(index,{args:e.target.value?e.target.value.split(/\r?\n/):[]})}/>
            <AssertionFields id={"stage-assertions-"+index} value={stage.expectedAssertions} onChange={expectedAssertions=>changeStage(index,{expectedAssertions})}/>
            <label htmlFor={"stage-timeout-"+index}>检查时限（秒）</label><input id={"stage-timeout-"+index} type="number" required min={1} max={3600} value={stage.timeoutSeconds} onChange={e=>changeStage(index,{timeoutSeconds:Number(e.target.value)})}/>
            <div className="card-footer"><button className="secondary-button" type="button" disabled={index===0} onClick={()=>moveStage(index,-1)}>上移</button><button className="secondary-button" type="button" disabled={index===stages.length-1} onClick={()=>moveStage(index,1)}>下移</button><button className="secondary-button" type="button" disabled={stages.length===1} onClick={()=>changeStages(stages.filter((_,i)=>i!==index))}>删除检查</button></div>
          </div>)}
          <button className="secondary-button" type="button" disabled={stages.length>=8} onClick={()=>changeStages([...stages,{id:"check-"+crypto.randomUUID(),name:"检查 "+(stages.length+1),validatorId:data.validators[0]?.id??"",args:[],timeoutSeconds:30}])}>添加验收检查</button>
          <p className="secondary">{stages.length} 项检查 · 时限合计 {totalSeconds} 秒 / 总上限 {draft.maxSeconds} 秒</p>
          {totalSeconds>draft.maxSeconds&&<p role="alert">请调整各项时限或总上限后保存。</p>}
          <label htmlFor="planning-artifacts">必须交付的文件（项目相对路径，每行一个）</label><textarea id="planning-artifacts" required rows={3} value={artifacts} onChange={e=>{setArtifacts(e.target.value);setDirty(true);}} placeholder="src/feature.ts" />
          <div className="planning-fields">
            <div><label htmlFor="planning-worker-limit">编程请求上限</label><input id="planning-worker-limit" type="number" required min={1} max={1000} value={draft.workerMaxRequests} onChange={e=>update("workerMaxRequests",Number(e.target.value))}/></div>
            <div><label htmlFor="planning-management-limit">项目主副指挥累计请求上限</label><input id="planning-management-limit" type="number" required min={1} max={1000} value={draft.managementMaxRequests} onChange={e=>update("managementMaxRequests",Number(e.target.value))}/></div>
            <div><label htmlFor="planning-seconds">单次执行时限（秒）</label><input id="planning-seconds" type="number" required min={10} max={3600} value={draft.maxSeconds} onChange={e=>update("maxSeconds",Number(e.target.value))}/></div>
          </div>
          <section className="panel" aria-label="异常监督配置">
            <h3>项目默认异常监督</h3>
            <label className="checkbox-label"><input type="checkbox" disabled={!draft.supervision&&!canRecordSupervision(defaultAdapter)} checked={!!draft.supervision} onChange={e=>update("supervision",e.target.checked?{schemaVersion:1,repeatedFailureThreshold:2,maxAdvicePerAttempt:0}:undefined)}/>{defaultAdapter==="omp"?"记录连续文件工具失败":"记录连续命令失败"}</label>
            {draft.supervision&&<div className="planning-fields">
              <div><label htmlFor="supervision-threshold">{defaultAdapter==="omp"?"同一文件工具连续失败阈值":"同一命令连续失败阈值"}</label><input id="supervision-threshold" type="number" min={2} max={8} required value={draft.supervision.repeatedFailureThreshold} onChange={e=>update("supervision",{...draft.supervision!,repeatedFailureThreshold:Number(e.target.value)})}/></div>
              <div><label htmlFor="supervision-advice-limit">每次尝试建议上限</label><input id="supervision-advice-limit" type="number" min={0} max={2} required value={draft.supervision.maxAdvicePerAttempt} onChange={e=>update("supervision",{...draft.supervision!,maxAdvicePerAttempt:Number(e.target.value),...(Number(e.target.value)===0?{deliveryEnabled:false}:{})})}/></div>
            </div>}
            {draft.supervision&&<label className="checkbox-label"><input type="checkbox" checked={!!draft.supervision.deliveryEnabled} disabled={!draft.supervision.deliveryEnabled&&(draft.supervision.maxAdvicePerAttempt===0||!canRecordSupervision(defaultAdapter))} onChange={e=>update("supervision",{...draft.supervision!,deliveryEnabled:e.target.checked})}/>允许向当前执行轮次发送建议</label>}
            {!canRecordSupervision(defaultAdapter)&&<p className="secondary">默认执行器尚不支持异常监督。支持监督的附加执行配置仍可单独设置。</p>}
            {canRecordSupervision(defaultAdapter)&&defaultAdapter!=="codex"&&defaultAdapter!=="omp"&&<p className="secondary">允许投送时，API 在工具执行后等待建议，并将建议计入下一轮请求。未允许投送时只保存建议供审阅。</p>}
            {defaultAdapter==="omp"&&<p className="secondary">OMP 根据原文件工具的连续失败请求副指挥建议，计入管理请求额度。允许投送时，在原文件工具结果等待期间发送一次建议，并核对原任务队列与消息流；结果未知不重发。未允许投送时只保存建议供审阅。</p>}
            {defaultSupervisionConflict&&<p role="alert">默认执行器不能使用当前监督策略。请关闭默认监督，或改用支持监督的执行器。</p>}
            <p className="secondary">默认执行器使用此策略。附加配置可以沿用、关闭或单独设置监督，修改其中一个不会清空其他配置。</p>
            <p className="secondary">0 条表示只记录异常。达到阈值可自动请求并保存副指挥建议，计入项目累计请求上限。发送建议须单独勾选并随方案批准；未知回执不会重发。修改监督配置需重新规划和批准。</p>
          </section>
          <p className="secondary">请求次数上限不等于费用上限。主副指挥分别使用所选接入，API 费用由对应供应商计收。{desktopAdapter?"桌面执行沿用所选应用的账号和模型；保存配置不会向该聊天发送消息。":draft.apiWorker?"API 执行使用接入管理中对应供应商的本机加密 Key；保存配置不发出模型请求。":draft.workerAdapter==="omp"?"OMP 使用本机保存的 DeepSeek API Key；程序应位于项目目录之外。":draft.chiefDirectory?"编程执行使用该模型独立 Windows 账户的 Codex 登录。":"编程执行使用本机 Codex 登录。"}</p>
          {supervisionConflict&&<p role="alert">请先修正默认或附加执行配置中的监督冲突，再保存。</p>}
          <button className="secondary-button" disabled={retiredAdapter||!data.validators.length||totalSeconds>draft.maxSeconds||supervisionConflict||!!budgetError}>{locked?"保存为下一版草稿":"保存需求与执行配置"}</button>
        </fieldset>
      </form>
      {locked && <p className="secondary">已有批准的任务，仍可编辑并保存后续模型配置草稿。原尝试保持原模型与目录；任务收口后请求方案修订，再保存下一版配置、重新规划并批准。</p>}
      <button className="secondary-button" type="button" disabled={busy} onClick={loadNextDraft}>载入下一版草稿</button>
      {draftMessage&&<p role="status">{draftMessage}</p>}
      {saved&&<WorkerAccountControls saved={saved} data={data} disabled={busy||pending||locked||dirty} send={send}/>}
      <div className="card-footer"><p className="secondary">主副指挥请求已记录 {requestCount} 次{saved?` / 上限 ${saved.setup.managementMaxRequests} 次`:""}。包含全部历史与已放弃请求，保存配置不会清零。</p><button className="primary" disabled={retiredAdapter||busy||dirty||!saved||!!saved?.setup.policy.currencyBudget||pending||locked||(managementKeyMissing(data,saved?.setup.managementSources?.chief))||requestCount>=(saved?.setup.managementMaxRequests??0)} onClick={()=>{if(saved)void send({type:"requestChiefPlan",projectId,setupDigest:saved.digest});}}>请求主指挥生成方案</button></div>
      {saved?.setup.policy.currencyBudget && <section className="panel" aria-label="规划金额许可">
        <p>主指挥生成方案也会产生费用。请先审阅规划预算许可，后续执行仍须批准具体方案，同一项目的费用和预留不会清零。</p>
        {data.currencyBudgets?.some(a=>a.projectId===projectId&&a.planningSetupDigest===saved.digest&&a.approvalLinked)&&<p role="status">当前配置的规划预算许可已记录。</p>}
        <button type="button" className="secondary-button" disabled={retiredAdapter||busy||dirty||pending||locked} onClick={()=>setPlanningBudgetReview(saved)}>审阅规划预算</button>
      </section>}
      {managementKeyMissing(data,saved?.setup.managementSources?.chief) && <p className="secondary">请在“接入管理”配置所选主指挥供应商的本机凭证后发起规划；配置可以先保存。</p>}
      {decisions.map(d=><div key={d.operationId} className="planning-status"><strong>{states[d.state]??d.state}</strong>{d.message&&<p>{d.message}</p>}{d.state==="unknown"&&<p>没有可确认的回复，系统不会自动再次发送。</p>}</div>)}
    </>}
    {planningBudgetReview?.setup.policy.currencyBudget&&<div className="modal-backdrop"><section className="modal panel execution-review" role="dialog" aria-modal="true" aria-label="确认主指挥规划预算">
      <h2>确认主指挥规划预算</h2>
      <p>仅授权当前配置下主指挥 {planningBudgetReview.setup.chiefModel} 的规划费用，不批准执行方案或派发编程任务。当前版本只记录许可，确认后不会发送模型请求。</p>
      <p>需求：{planningBudgetReview.setup.requirement}</p>
      <CurrencyBudgetReview value={planningBudgetReview.setup.policy.currencyBudget}/>
      <details><summary>配置引用</summary><code>{planningBudgetReview.digest}</code></details>
      {(dirty||saved?.digest!==planningBudgetReview.digest)&&<p role="alert">配置已变化，请返回并重新审阅。</p>}
      <div className="card-footer"><button className="secondary-button" onClick={()=>setPlanningBudgetReview(undefined)}>返回配置</button>
        <button className="primary" disabled={retiredAdapter||busy||dirty||pending||locked||saved?.digest!==planningBudgetReview.digest} onClick={async()=>{
          if(!dirty&&saved?.digest===planningBudgetReview.digest&&await send({type:"authorizePlanningBudget",projectId:planningBudgetReview.setup.projectId,setupDigest:planningBudgetReview.digest}))setPlanningBudgetReview(undefined);
        }}>确认规划预算</button></div>
    </section></div>}
  </section>;
}
