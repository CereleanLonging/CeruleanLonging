import { HelpHeading } from "./HelpHeading";
import { WorkerAccountControls } from "./WorkerAccountControls";
import { MarkdownImport } from "./MarkdownImport";
import { AssertionFields, cleanAssertions } from "./AssertionFields";
import { ManagementSourceFields } from "./ManagementSourceFields";
import { WorkflowTemplateImport } from "./WorkflowTemplateImport";
import { IntegrationHub, type ConfigurationRequest } from "./IntegrationHub";
import { ProjectConnections } from "./ProjectConnections";
import { defaultWorkerAdapter, revealConnectionField, workerMethod, type WorkerMethod } from "./ConnectionMethods";
import { CodexConnection } from "./CodexConnection";
import { useEffect, useRef, useState } from "react";
import { flushAccessStorage, useAccessPersistence } from "./accessStorage";
import { accessKey, loadAccessDraft, saveAccessDraft } from "./accessDraft";
import { preferenceKey, readPreference, writePreference } from "./accessPreferences";
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
export function ProjectPlanning({data,busy,send,configuration,onConfigurationReady,onSetup,mode="all",onContinue,onConnections}:{data:Snapshot|null;busy:boolean;send:(command:Command)=>Promise<boolean>;configuration?:ConfigurationRequest|null;onConfigurationReady?:(token:number)=>void;onSetup?:()=>void;mode?:"all"|"connections"|"planning";onContinue?:()=>void;onConnections?:()=>void}) {
  const [projectId,setProjectId]=useState("");
  const [draft,setDraft]=useState<ProjectSetupDraft>(empty);
  const [reviewedAccess,setReviewedAccess]=useState<string|null>(null);
  const durable=useAccessPersistence();
  const [accessChecking,setAccessChecking]=useState(false);
  const accessCheckLock=useRef(false);
  const connectionFields=useRef<HTMLFieldSetElement>(null);
  const [hydratedContext,setHydratedContext]=useState("");
  const lastAccessWrite=useRef("");
  const [accessNotice,setAccessNotice]=useState("");
  const [accessSaveError,setAccessSaveError]=useState(false);
  const connections=mode==="connections";
  const accessReady=reviewedAccess===accessKey(draft);
  const [budgetError,setBudgetError]=useState<string>();
  const [draftMessage,setDraftMessage]=useState("");
  const draftKey=JSON.stringify(["cerulean-next-execution",data?.dataDirectory,projectId]);
  function keepNextDraft(){try{localStorage.setItem(draftKey,JSON.stringify({draft,artifacts,stages,basedOn:saved?.digest}));setDraftMessage("下一版草稿已保存在本机；当前任务保持原模型和目录。");}catch{setDraftMessage("无法保存本机草稿，请保留此窗口。");}}
  function loadNextDraft(){try{const x=JSON.parse(localStorage.getItem(draftKey)??"null");if(!x||x.draft?.projectId!==projectId||typeof x.artifacts!=="string"||!Array.isArray(x.stages))throw new Error();setDraft(x.draft);setArtifacts(x.artifacts);setStages(x.stages);setDirty(true);setDraftMessage("已载入下一版草稿，请重新审阅后保存配置。");}catch{setDraftMessage("没有可载入的下一版草稿。");}}
  const [planningBudgetReview,setPlanningBudgetReview]=useState<SavedProjectSetup>();
  const [artifacts,setArtifacts]=useState("");const [stages,setStages]=useState<AcceptanceStageDraft[]>([]);const [dirty,setDirty]=useState(false);
  const saved=data?.projectSetups?.find(s=>s.setup.projectId===projectId);
  const projectRoot=data?.workflows.find(w=>w.project.id===projectId)?.project.root??"";
  const storageEnabled=mode!=="all"&&!!data?.dataDirectory&&!!projectId&&!!projectRoot;
  const accessContext=JSON.stringify([data?.dataDirectory,projectId,projectRoot,saved?.digest??null]);
  useEffect(()=>{
    const selected=data?.workflows.find(w=>w.project.id===configuration?.projectId)?.project.id;
    const remembered=readPreference(preferenceKey(data?.dataDirectory,"planning-project"));
    if(selected&&selected!==projectId)setProjectId(selected);
    else if(!data?.workflows.some(w=>w.project.id===projectId)&&data?.workflows.length)
      setProjectId(data.workflows.find(w=>w.project.id===remembered)?.project.id??data.workflows[0].project.id);
  },[projectId,data?.workflows,configuration?.projectId]);
  useEffect(()=>{if(storageEnabled)writePreference(preferenceKey(data?.dataDirectory,"planning-project"),projectId);},[storageEnabled,data?.dataDirectory,projectId]);
  useEffect(()=>{
    const s=saved?.setup;
    const nextDraft:ProjectSetupDraft = s ? {chiefDirectory:s.chiefDirectory,managementSources:s.managementSources,currencyBudget:s.policy.currencyBudget,resourceKeys:s.resourceKeys,profiles:s.profiles?.map(restoreProfile),worktree:s.worktree?{integration:s.worktree.integration?{taskId:s.worktree.integration.taskId,artifactPaths:s.worktree.integration.artifactPaths,validatorId:s.worktree.integration.validatorId,args:s.worktree.integration.check.args,timeoutSeconds:s.worktree.integration.check.timeoutSeconds}:undefined,inheritDependencies:s.worktree.input==="committed-raw-blobs-with-accepted-dependencies"?true:undefined,baseCommit:s.worktree.baseCommit,poolRoot:s.worktree.poolRoot,gitExecutable:s.worktree.git.path}:undefined,workerAdapter:s.workerAdapter??"codex",apiWorker:s.apiWorker,desktopWorker:s.desktopWorker,ompRuntimeParent:s.worker?.ompRuntimeParent,supervision:s.policy.supervision,projectId,requirement:s.requirement,codexExecutable:s.worker?.executable.path??"",workerModel:s.desktopWorker?.modelLabel??s.apiWorker?.model??s.worker?.model??"",chiefModel:s.chiefModel,deputyModel:s.deputyModel,validatorId:s.validatorId,checkArgs:s.check.args,artifactPaths:s.artifactPaths,workerMaxRequests:s.policy.maxRequests,managementMaxRequests:s.managementMaxRequests,maxSeconds:s.policy.maxSeconds} : {...empty,projectId,chiefDirectory:(data?.workflows.find(w=>w.project.id===projectId)?.project.root??"")+"-chief",codexExecutable:data?.codexConnection?.executable??"",validatorId:data?.validators[0]?.id??""};
    const restored=storageEnabled&&data?loadAccessDraft(data,projectId,projectRoot,saved?.digest??null):{state:"empty" as const};
    const next=restored.state==="restored"?{...nextDraft,...restored.values}:nextDraft;
    const checked=restored.state==="restored"?restored.fieldsChecked:!!s;
    const reviewed=checked?accessKey(next):null;
    setDraft(next);setReviewedAccess(reviewed);setHydratedContext(accessContext);
    lastAccessWrite.current=JSON.stringify([accessContext,accessKey(next),reviewed]);
    setDirty(accessKey(next)!==accessKey(nextDraft));setAccessSaveError(restored.state==="invalid");
    setAccessNotice(restored.state==="restored"?(checked?"已恢复自动保存的接入配置，格式检查已通过；可直接修改。":"已恢复自动保存的接入草稿，请检查后继续。"):
      restored.state==="invalid"?"本机接入草稿无法读取，已保留原记录；请检查当前配置并重新保存。":
      restored.state==="superseded"?"项目配置已更新，已载入最新配置；旧接入草稿不会覆盖新版本。":"");
    setArtifacts(s?.artifactPaths.join("\n")??"");
    setStages(s?.acceptance?.stages.map(stage=>({expectedAssertions:stage.check.expectedAssertions,id:stage.check.id,name:stage.check.name??stage.check.id,validatorId:stage.validatorId,args:stage.check.args,timeoutSeconds:stage.check.timeoutSeconds}))??[{expectedAssertions:s?.check.expectedAssertions,id:s?.check.id??"project-check",name:s?.check.name??"项目验收",validatorId:s?.validatorId??data?.validators[0]?.id??"",args:s?.check.args??[],timeoutSeconds:s?.check.timeoutSeconds??60}]);
  },[accessContext]);
  function persistAccess(checked:boolean) {
    if(!storageEnabled||!data)return true;
    const ok=saveAccessDraft(data.dataDirectory,projectRoot,saved?.digest??null,draft,checked);
    setAccessSaveError(!ok);
    setAccessNotice(ok?(checked?"格式检查通过，接入配置已自动保存；可随时修改。":"接入草稿已自动保存，修改后请重新检查。"):
      "接入配置自动保存失败，输入仍保留在当前窗口；请重试保存后再关闭。");
    return ok;
  }
  const currentAccess=accessKey(draft);
  useEffect(()=>{
    if(!storageEnabled||hydratedContext!==accessContext||draft.projectId!==projectId)return;
    const stamp=JSON.stringify([accessContext,currentAccess,reviewedAccess]);
    if(stamp===lastAccessWrite.current)return;
    lastAccessWrite.current=stamp;
    persistAccess(currentAccess===reviewedAccess);
  },[storageEnabled,hydratedContext,accessContext,currentAccess,reviewedAccess]);
  async function validateAccess() {
    if(busy||pending||retiredAdapter||accessCheckLock.current)return false;
    const invalid=Array.from(connectionFields.current?.querySelectorAll<HTMLInputElement|HTMLSelectElement|HTMLTextAreaElement>("input,select,textarea")??[])
      .find(field=>!field.closest("[data-connection-actions]")&&!field.matches(":disabled")&&!field.checkValidity());
    if(invalid){revealConnectionField(invalid);invalid.reportValidity();return false;}
    if(resourceError(draft.resourceKeys)||(draft.profiles??[]).some(p=>resourceError(p.resourceKeys,draft.resourceKeys))||supervisionConflict){
      setAccessNotice("请先修正接入配置中的资源或监督冲突，再检查保存。");setAccessSaveError(true);return false;
    }
    accessCheckLock.current=true;setAccessChecking(true);
    try {
      if(!persistAccess(true)||!await flushAccessStorage())return false;
      lastAccessWrite.current=JSON.stringify([accessContext,currentAccess,currentAccess]);
      setReviewedAccess(currentAccess);return true;
    }finally{accessCheckLock.current=false;setAccessChecking(false);}
  }
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
  useEffect(()=>{
    if(!configuration||!projectId||draft.projectId!==projectId||busy||pending)return;
    if(configuration.role==="chief"&&configuration.chiefMethod){
      const sources=draft.managementSources??{chief:{kind:"openAi" as const},deputy:{kind:"openAi" as const}};
      const desktop=configuration.chiefMethod==="desktop";
      if(desktop!==(sources.chief.kind==="codexSession")){
        setDraft(d=>({...d,chiefModel:desktop?"external-codex-session":"",managementSources:{...sources,chief:desktop?{kind:"codexSession",sessionId:""}:{kind:"openAi"}}}));
        setDirty(true);
      }
    }
    const frame=requestAnimationFrame(()=>{
      const field=document.getElementById(`planning-${configuration.role==="worker"?"adapter":configuration.role+"-source"}`);
      if(!field)return;
      field.closest(".role-model-fields, .integration-role")?.scrollIntoView({block:"start",behavior:"smooth"});
      if(connections)field.closest(".integration-role")?.querySelector("summary")?.focus();
      else field.focus();
      onConfigurationReady?.(configuration.token);
    });
    return()=>cancelAnimationFrame(frame);
  },[configuration,projectId,draft.projectId,busy,pending]);
  function chooseChiefMethod(method: "api" | "desktop") {
    const sources = draft.managementSources ?? {chief:{kind:"openAi" as const},deputy:{kind:"openAi" as const}};
    const desktop = method === "desktop";
    if (desktop === (sources.chief.kind === "codexSession")) return;
    setDraft({...draft, chiefModel:desktop?"external-codex-session":"", managementSources:{...sources,
      chief:desktop?{kind:"codexSession",sessionId:""}:{kind:"openAi"}}});
    setDirty(true);
  }
  function chooseDefaultWorker(method: WorkerMethod) {
    const adapter = defaultWorkerAdapter[method];
    setDraft({...draft, workerAdapter:adapter, apiWorker:newApiWorker(adapter), desktopWorker:undefined,
      codexExecutable:adapter==="codex"?(data?.codexConnection?.executable??""):"", workerModel:"",
      ompRuntimeParent:undefined, worktree:method==="desktop"?undefined:draft.worktree});
    setDirty(true);
  }
  const selectedWorkerMethod=workerMethod(draft.workerAdapter);
  const FormContainer = connections ? "div" : "form";
  const workerFields = data ? <section className={connections?"role-configuration-fields":"role-model-fields"} aria-label="执行端模型配置">{!connections&&<HelpHeading as="h3" topic="ProjectPlanning">执行端</HelpHeading>}
          <div className="planning-fields">
            <div><label htmlFor="planning-adapter">编程执行器</label><select id="planning-adapter" value={draft.workerAdapter??"codex"} onChange={e=>{setDraft(d=>({...d,workerAdapter:e.target.value as ProjectWorkerAdapter,worktree:["qoder","workbuddy"].includes(e.target.value)?undefined:d.worktree,apiWorker:newApiWorker(e.target.value as ProjectWorkerAdapter),desktopWorker:undefined,codexExecutable:"",workerModel:"",ompRuntimeParent:undefined}));setDirty(true);}}>{(!connections||selectedWorkerMethod==="cli")&&<><option value="codex">Codex</option><option value="omp">OMP · DeepSeek</option></>}
              {(!connections||selectedWorkerMethod==="desktop")&&<><option value="qoder">Qoder · 已有桌面会话</option>{draft.workerAdapter==="workbuddy"&&<option value="workbuddy" disabled>WorkBuddy · 支持已移除</option>}</>}
              {(!connections||selectedWorkerMethod==="api")&&apiWorkerOptions.map(([id,label])=><option key={id} value={id}>{label}</option>)}</select></div>
            {connections&&draft.workerAdapter==="codex"&&<CodexConnection scope={data.dataDirectory} status={data.codexConnection} send={send} busy={busy} connected value={draft.codexExecutable} onChange={value=>update("codexExecutable",value)} idPrefix="planning"/>}
            {!draft.apiWorker&&!desktopAdapter&&!(connections&&draft.workerAdapter==="codex")&&<div><label htmlFor="planning-executable">编程程序路径</label><input id="planning-executable" required value={draft.codexExecutable} onChange={e=>update("codexExecutable",e.target.value)} placeholder={draft.workerAdapter==="omp"?"OMP 可执行文件的绝对路径":"Codex 可执行文件的绝对路径"} /></div>}
            {!desktopAdapter&&<div><label htmlFor="planning-worker">编程模型 ID</label><input id="planning-worker" required list="planning-worker-models" value={draft.workerModel} onChange={e=>{const model=e.target.value;setDraft(d=>({...d,workerModel:model,apiWorker:d.apiWorker?{...d.apiWorker,model}:undefined}));setDirty(true);}} /></div>}
          </div>
          {draft.workerAdapter==="workbuddy"&&<p role="alert">WorkBuddy 支持已移除，历史配置保留。请选择其他执行器后保存新版配置。</p>}
          {draft.workerAdapter==="qoder"&&<DesktopWorkerFields isolationEnabled={!!draft.chiefDirectory} data={data} projectId={projectId} app="qoder" send={send} busy={busy} worker={draft.desktopWorker} onChange={worker=>{setDraft(d=>({...d,desktopWorker:worker,workerModel:worker?.modelLabel??""}));setDirty(true);}}/>}
          {draft.apiWorker&&<ApiWorkerFields worker={draft.apiWorker} onChange={api=>update("apiWorker",api)}/>}
          <NativeWorkerCost adapter={draft.workerAdapter??"codex"}/>
          {draft.workerAdapter==="omp"&&(draft.chiefDirectory?<p className="secondary">OMP 运行配置固定保存在该模型目录内，与其他执行模型隔离。</p>:<><label htmlFor="planning-omp-runtime">OMP 独立运行目录（可选）</label><input id="planning-omp-runtime" value={draft.ompRuntimeParent??""} onChange={e=>update("ompRuntimeParent",e.target.value||undefined)} placeholder="已存在、可写的专用目录绝对路径"/><p className="secondary">用户目录含其他 Agent 配置时，请选择独立位置。须与项目和工作台数据目录分开，祖先目录不能含隐含配置。留空沿用工作台数据目录；更改后需重新规划和批准，历史运行文件保留。</p></>)}
          <datalist id="planning-chief-models">{managementModelList(data,draft.managementSources?.chief).map(m=><option key={m} value={m}/>)}</datalist><datalist id="planning-deputy-models">{managementModelList(data,draft.managementSources?.deputy).map(m=><option key={m} value={m}/>)}</datalist><datalist id="planning-worker-models">{(draft.workerAdapter??"codex")==="codex"&&data.codexConnection?.models.map(m=><option key={m} value={m}/>)}</datalist>
          <WorktreeFields disabledReason={draft.chiefDirectory?"已启用固定模型目录，不与按尝试创建的 Git 目录池混用。":undefined} validators={data.validators} value={draft.worktree??undefined} disabled={!!draft.chiefDirectory||desktopAdapter||(draft.profiles??[]).some(p=>p.workerAdapter==="qoder"||p.workerAdapter==="workbuddy")} onChange={value=>update("worktree",value)}/>
          <ResourceFields id="planning-resources" value={draft.resourceKeys} onChange={value=>update("resourceKeys",value)}/>
          {!connections&&<WorkerProfiles send={send} busy={busy} showConnections={connections} isolationEnabled={!!draft.chiefDirectory} supervision={draft.supervision} commonResources={draft.resourceKeys??[]} data={data} projectId={projectId} value={draft.profiles??[]} worktree={!!draft.worktree} onChange={profiles=>{setDraft(d=>({...d,profiles:profiles.length?profiles:undefined}));setDirty(true);}}/>}
          </section> : null;
  return <section className="panel project-planning" aria-label={connections?"项目角色接入配置":"主指挥项目规划"}>
    <div className="section-heading"><HelpHeading as="h2" topic="ProjectPlanning">{connections?"凭证与连接检查":"向主指挥提交需求"}</HelpHeading><span className="badge">{connections?((accessSaveError||durable.error)?"保存需处理":durable.pending?"正在保存":accessReady?"接入格式已检查":"接入待检查"):saved?`配置版本 ${saved.setup.version}`:"尚未配置"}</span></div>
    {!data?.workflows.length ? <><p className="secondary">请先关联项目目录，再填写主指挥来源和会话标识；独立验收工具已固定关联，保存执行配置前还需登记项目功能检查程序。</p>{onSetup&&<button className="secondary-button" disabled={busy} onClick={onSetup}>关联项目并配置主指挥</button>}{connections&&data&&<IntegrationHub data={data} busy={busy} send={send} configure={()=>onSetup?.()}/>}</> : <>
      <label htmlFor="planning-project">{connections?"接入项目":"规划项目"}</label><select id="planning-project" value={projectId} disabled={busy||pending||accessChecking} onChange={e=>setProjectId(e.target.value)}>{data.workflows.map(w=><option value={w.project.id} key={w.project.id}>{w.project.root}</option>)}</select>
      <p className="secondary">{connections?"第 1 步：选择主指挥、副指挥与执行端。填写内容会按项目自动保存，下次打开自动恢复，可随时修改。检查后再填写需求与计划。":"第 2 步：填写需求、交付范围与验收检查，保存后再请求主指挥提出方案。方案仍须审阅批准。"} </p>
      {mode==="planning"&&<section className="panel" aria-label="已选角色模型"><HelpHeading as="h3" topic="ProjectPlanning">接入配置摘要</HelpHeading>
        <p>主指挥：{draft.chiefModel||"未选择"} · 副指挥：{draft.deputyModel||"未选择"} · 执行端：{draft.workerModel||"未选择"}</p>
        {!accessReady&&<p role="status">请先在接入管理确认本项目的角色模型，再保存需求与计划。</p>}
        <button className="secondary-button" type="button" disabled={busy||pending} onClick={onConnections}>修改接入配置</button>
      </section>}
      {connections&&accessNotice&&<p role={accessSaveError||durable.error?"alert":"status"}>{durable.error|| (durable.pending?"正在保存接入设置到本机…":accessNotice)}</p>}
      <FormContainer onSubmit={connections ? undefined : async e=>{e.preventDefault();if(busy||pending)return;if(connections){if(await validateAccess())onContinue?.();return;}if(mode==="planning"&&!accessReady){onConnections?.();return;}if(locked){keepNextDraft();return;}if(budgetError||retiredAdapter)return;if(resourceError(draft.resourceKeys)||(draft.profiles??[]).some(p=>resourceError(p.resourceKeys,draft.resourceKeys)))return;if(await send({type:"configureProject",draft:{...draft,resourceKeys:resourceNames(draft.resourceKeys).length?resourceNames(draft.resourceKeys):undefined,profiles:draft.profiles?.map(p=>({...p,acceptance:{...p.acceptance,stages:p.acceptance.stages.map(s=>({...s,expectedAssertions:cleanAssertions(s.expectedAssertions)}))},resourceKeys:resourceNames(p.resourceKeys).length?resourceNames(p.resourceKeys):undefined})),projectId,artifactPaths:artifacts.split(/\r?\n/).map(v=>v.trim()).filter(Boolean),validatorId:stages[0]?.validatorId??"",checkArgs:stages[0]?.args??[],acceptance:{schemaVersion:1,stages:stages.map(s=>({...s,expectedAssertions:cleanAssertions(s.expectedAssertions)}))}}})){setDirty(false);localStorage.removeItem(draftKey);setDraftMessage("");}}}>
        <fieldset disabled={busy||pending||accessChecking}>
          <fieldset className="planning-step-fields" hidden={connections} disabled={connections}>
          <WorkflowTemplateImport contextKey={JSON.stringify([projectId,saved?.digest,draft])} disabled={busy||pending} onApply={(requirement,supervision)=>{setDraft(d=>({...d,requirement,...(supervision?{supervision}:{})}));setDirty(true);}}/>
          <label htmlFor="planning-requirement">项目需求</label><textarea id="planning-requirement" required rows={4} value={draft.requirement} onChange={e=>update("requirement",e.target.value)} placeholder="描述要解决的问题、预期行为和完成标准。" />
          <MarkdownImport chief={saved} mode="requirement" contextKey={JSON.stringify([projectId,saved?.digest,draft])} disabled={busy||pending} onApply={text=>update("requirement",text)}/>
          </fieldset>
          <fieldset ref={connectionFields} className="planning-step-fields" hidden={mode==="planning"} disabled={mode==="planning"}>
          {connections ? <ProjectConnections key={projectId} data={data} busy={busy} send={send} draft={draft}
            chief={<ManagementSourceFields role="chief" embedded method={draft.managementSources?.chief.kind==="codexSession"?"desktop":"api"} draft={draft} onChange={next=>{setDraft(next);setDirty(true);}}/>}
            deputy={<ManagementSourceFields role="deputy" embedded method="api" draft={draft} onChange={next=>{setDraft(next);setDirty(true);}}/>}
            worker={workerFields} onChiefMethod={chooseChiefMethod} onDefaultWorker={chooseDefaultWorker}
            profiles={method=><WorkerProfiles methodFilter={method} send={send} busy={busy} showConnections
              isolationEnabled={!!draft.chiefDirectory} supervision={draft.supervision} commonResources={draft.resourceKeys??[]}
              data={data} projectId={projectId} value={draft.profiles??[]} worktree={!!draft.worktree}
              onChange={profiles=>{setDraft(d=>({...d,profiles:profiles.length?profiles:undefined}));setDirty(true);}}/>}/> : <>
          <div className="planning-fields">
            <ManagementSourceFields draft={draft} onChange={next=>{setDraft(next);setDirty(true);}}/>
          </div>
            {workerFields}
          </>}
          <details className="integration-group project-directories" open={connections?undefined:true}>
            <summary>角色目录与隔离设置</summary>
          <section className="panel" aria-label="角色与模型工作目录"><HelpHeading as="h3" topic="ProjectPlanning">角色与模型工作目录</HelpHeading>
            <label htmlFor="planning-chief-directory">主指挥独立目录</label><input id="planning-chief-directory" readOnly={!!saved?.setup.chiefDirectory} value={draft.chiefDirectory??""} onChange={e=>{update("chiefDirectory",e.target.value||undefined);if(e.target.value)update("worktree",undefined);}} placeholder={(workflow?.project.root??"")+"-chief"}/>
            <p>副指挥与执行端总目录：<code>{workflow?.project.root}</code></p>
            <p className="secondary">副指挥只读调取主指挥原需求与方案。执行模型首次接入时，在总目录的 executors 下建立独立子目录；最多五个，切回已有模型复用原目录。模型切换不能改变正在运行尝试的目录。</p>
            <p className="secondary">正常执行只能读写该模型的工作目录。不同执行端须负责不同源码文件；副指挥会收到目录与文件边界，冲突分工不能派发。评审只提供独立验收工具处理后的只读片段，不开放其他目录。</p>
            {!draft.chiefDirectory&&<p role="status">当前配置未绑定独立执行目录，不能启动任务。请填写主指挥独立目录，保存新版配置并重新规划、批准；历史记录仍保留。</p>}
            {saved?.setup.workerDirectories&&<ul>{Object.values(saved.setup.workerDirectories).map(d=>d&&<li key={d.identity}><code>{d.relativePath}</code></li>)}</ul>}
          </section>
          </details>
          </fieldset>
          <fieldset className="planning-step-fields" hidden={connections} disabled={connections}>
          <CurrencyBudgetFields resetKey={projectId+":"+(saved?.digest??"")} resetValue={saved?.setup.policy.currencyBudget} draft={draft} value={draft.currencyBudget} onChange={value=>update("currencyBudget",value)} onInvalid={setBudgetError}/>
          <HelpHeading as="h3" topic="ProjectPlanning">有序验收检查</HelpHeading>
          <p className="secondary">按以下顺序执行，全部通过后才可收口。每项使用已登记工具，参数按行传递。</p>
          {stages.map((stage,index)=><div className="panel" key={stage.id}>
            <div className="section-heading"><strong>第 {index+1} 项</strong></div>
            <label htmlFor={"stage-name-"+index}>检查名称</label><input id={"stage-name-"+index} required value={stage.name} onChange={e=>changeStage(index,{name:e.target.value})}/>
            <label htmlFor={index===0?"planning-validator":"stage-validator-"+index}>项目功能检查程序</label><select id={index===0?"planning-validator":"stage-validator-"+index} required value={stage.validatorId} onChange={e=>changeStage(index,{validatorId:e.target.value})}><option value="">选择已登记检查程序</option>{data.validators.map(v=><option key={v.id} value={v.id}>{v.name}</option>)}</select>
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
            <HelpHeading as="h3" topic="ProjectPlanning">项目默认异常监督</HelpHeading>
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
          <button className="secondary-button" disabled={retiredAdapter||!data.validators.length||totalSeconds>draft.maxSeconds||supervisionConflict||!!budgetError||(mode==="planning"&&!accessReady)}>{locked?"保存为下一版草稿":"保存需求与执行配置"}</button>
          </fieldset>
          {connections&&<div className="card-footer access-save-actions">
            {accessNotice&&<p className="secondary">{accessSaveError||durable.error?"保存失败，请重试。":durable.pending?"正在保存到本机…":accessReady?"接入已保存，可继续或直接修改。":"已保存草稿，待格式检查。"}</p>}
            <p className="secondary">自动保存接入选项；检查仅校验必填项与格式，连接可用性以单独的连接检查为准。凭证可在各角色的 Key 管理中加密保存，项目配置与需求一起提交。</p>
            <button type="button" className="secondary-button" disabled={busy||pending||accessChecking||retiredAdapter||supervisionConflict} onClick={()=>validateAccess()}>检查并保存接入配置</button>
            <button type="button" className="primary" disabled={busy||pending||accessChecking||retiredAdapter||supervisionConflict} onClick={async()=>{if(await validateAccess())onContinue?.();}}>下一步：设置需求与计划</button>
          </div>}
        </fieldset>
      </FormContainer>
      {locked && <p className="secondary">已有批准的任务，仍可编辑并保存后续模型配置草稿。原尝试保持原模型与目录；任务收口后请求方案修订，再保存下一版配置、重新规划并批准。</p>}
      <button className="secondary-button" type="button" disabled={busy} onClick={loadNextDraft}>载入下一版草稿</button>
      {draftMessage&&<p role="status">{draftMessage}</p>}
      {mode!=="planning"&&saved&&<WorkerAccountControls saved={saved} data={data} disabled={busy||pending||dirty} send={send}/>}
      {!connections&&<>
      <div className="card-footer"><p className="secondary">主副指挥请求已记录 {requestCount} 次{saved?` / 上限 ${saved.setup.managementMaxRequests} 次`:""}。包含全部历史与已放弃请求，保存配置不会清零。</p><button className="primary" disabled={retiredAdapter||busy||dirty||!saved||!!saved?.setup.policy.currencyBudget||pending||locked||(managementKeyMissing(data,saved?.setup.managementSources?.chief))||requestCount>=(saved?.setup.managementMaxRequests??0)} onClick={()=>{if(saved)void send({type:"requestChiefPlan",projectId,setupDigest:saved.digest});}}>请求主指挥生成方案</button></div>
      {saved?.setup.policy.currencyBudget && <section className="panel" aria-label="规划金额许可">
        <p>主指挥生成方案也会产生费用。请先审阅规划预算许可，后续执行仍须批准具体方案，同一项目的费用和预留不会清零。</p>
        {data.currencyBudgets?.some(a=>a.projectId===projectId&&a.planningSetupDigest===saved.digest&&a.approvalLinked)&&<p role="status">当前配置的规划预算许可已记录。</p>}
        <button type="button" className="secondary-button" disabled={retiredAdapter||busy||dirty||pending||locked} onClick={()=>setPlanningBudgetReview(saved)}>审阅规划预算</button>
      </section>}
      {managementKeyMissing(data,saved?.setup.managementSources?.chief) && <p className="secondary">请在“接入管理”配置所选主指挥供应商的本机凭证后发起规划；配置可以先保存。</p>}
      {decisions.map(d=><div key={d.operationId} className="planning-status"><strong>{states[d.state]??d.state}</strong>{d.message&&<p>{d.message}</p>}{d.state==="unknown"&&<p>没有可确认的回复，系统不会自动再次发送。</p>}</div>)}
      </>}
    </>}
    {!connections&&planningBudgetReview?.setup.policy.currencyBudget&&<div className="modal-backdrop"><section className="modal panel execution-review" role="dialog" aria-modal="true" aria-label="确认主指挥规划预算">
      <HelpHeading as="h2" topic="ProjectPlanning">确认主指挥规划预算</HelpHeading>
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
