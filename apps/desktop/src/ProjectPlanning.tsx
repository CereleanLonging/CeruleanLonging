import { HelpHeading } from "./HelpHeading";
import { invoke } from "@tauri-apps/api/core";
import { WorkerAccountControls } from "./WorkerAccountControls";
import { RequirementComposer } from "./RequirementComposer";
import { ChiefPlanRequest } from "./ChiefPlanRequest";
import { ManagementSourceFields } from "./ManagementSourceFields";
import { IntegrationHub, type ConfigurationRequest } from "./IntegrationHub";
import { ProjectConnections } from "./ProjectConnections";
import { ProjectAccessSummary } from "./ProjectAccessSummary";
import { defaultWorkerAdapter, localWorker, revealConnectionField, workerMethod, type WorkerMethod } from "./ConnectionMethods";
import { hasOmpKey, OmpConnectionTest, OmpKeyDialog } from "./OmpCredentials";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { flushAccessStorage, useAccessPersistence } from "./accessStorage";
import { accessContents, accessKey, loadAccessDraft, saveAccessDraft } from "./accessDraft";
import { preferenceKey, readPreference, writePreference } from "./accessPreferences";
import { managementKeyMissing, managementModelList } from "./ManagementProviders";
import { readApiModelDefault } from "./apiModels";
import { endpointProvider } from "./ManagementProviders";
import { automaticTaskDescription, suggestedWorkerResource, synchronizeWorkerProfiles, withAutomaticResource, workerConfigurationIdentity } from "./workerConfiguration";
import type { AcceptanceStageDraft, Command, ProjectSetupDraft, ProjectWorkerAdapter, Snapshot, SavedProjectSetup, WorkerProfileDraft } from "./contracts";

import { ApiWorkerFields, apiWorkerOptions, newApiWorker } from "./ApiWorkerFields";
import { DesktopWorkerFields } from "./DesktopWorkerFields";
import { NativeWorkerCost } from "./NativeWorkerCost";
import { WorkerProfiles, restoreProfile } from "./WorkerProfiles";
import { canRecordSupervision, defaultSupervision, effectiveSupervision, supervisionConflictFor } from "./SupervisionFields";
import { ResourceFields, resourceNames, resourceError } from "./ResourceFields";
import { WorktreeFields } from "./WorktreeFields";
import { CurrencyBudgetFields, CurrencyBudgetReview } from "./CurrencyBudgetFields";

const checkerId="cerulean-objective-checker";
const fixedStage=(seconds=60):AcceptanceStageDraft=>({id:"project-check",name:"独立客观检查",validatorId:checkerId,args:[],expectedAssertions:[],timeoutSeconds:seconds});
const empty:ProjectSetupDraft={workerAdapter:"deepseek",apiWorker:newApiWorker("deepseek"),projectId:"",requirement:"",codexExecutable:"",workerModel:"",chiefModel:"",deputyModel:"",validatorId:"",checkArgs:[],artifactPaths:[],workerMaxRequests:3,managementMaxRequests:10,maxSeconds:180};
function removeUnusedCodexDefault(value:ProjectSetupDraft, preferred?:ReturnType<typeof readApiModelDefault>):ProjectSetupDraft {
  // Old empty defaults were populated from the chief's installed executable.
  // Clear that placeholder only; configured historical workers remain visible
  // for explicit replacement and existing approved backend setups stay intact.
  if ((value.workerAdapter??"codex")!=="codex") return value;
  if (value.workerModel.trim()) return {...value,workerAdapter:"codex"};
  const adapter=preferred?(preferred.target.kind==="openAi"?"openai":endpointProvider(preferred.target.endpoint)):"deepseek";
  const apiWorker=newApiWorker(adapter)!;
  return {...value,workerAdapter:adapter,apiWorker:preferred?{...apiWorker,target:preferred.target,model:preferred.model}:apiWorker,codexExecutable:"",workerModel:preferred?.model??"",desktopWorker:undefined,ompRuntimeParent:undefined};
}
const states:Record<string,string>={awaitingExternal:"等待绑定的 Codex 聊天回填方案",running:"主指挥正在规划",ready:"回复待处理",applying:"方案正在入账",applied:"回复已处理",unknown:"请求结果待核对",discarded:"请求已放弃",abandoned:"应用事务已关闭",startupClosed:"启动记录已关闭"};
export function ProjectPlanning({data,busy,send,configuration,onConfigurationReady,onSetup,mode="all",onContinue,onConnections}:{data:Snapshot|null;busy:boolean;send:(command:Command,onError?:(message:string)=>void)=>Promise<boolean>;configuration?:ConfigurationRequest|null;onConfigurationReady?:(token:number)=>void;onSetup?:()=>void;mode?:"all"|"connections"|"planning";onContinue?:()=>void;onConnections?:()=>void}) {
  const [projectId,setProjectId]=useState("");
  const [draft,setDraft]=useState<ProjectSetupDraft>(empty);
  const [automaticResources,setAutomaticResources]=useState<Record<string,string>>({});
  const [allocationNotice,setAllocationNotice]=useState("");
  const [ompKeyPrompt,setOmpKeyPrompt]=useState(false);
  const [reviewedAccess,setReviewedAccess]=useState<string|null>(null);
  const durable=useAccessPersistence();
  const [accessChecking,setAccessChecking]=useState(false);
  const [accessReview,setAccessReview]=useState<{context:string;key:string}|null>(null);
  const accessReviewDialog=useRef<HTMLDialogElement>(null);
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
  function keepNextDraft(){try{localStorage.setItem(draftKey,JSON.stringify({draft,stages,basedOn:saved?.digest}));setDraftMessage("下一版草稿已保存在本机；当前任务保持原模型和目录。");}catch{setDraftMessage("无法保存本机草稿，请保留此窗口。");}}
  function loadNextDraft(){try{const x=JSON.parse(localStorage.getItem(draftKey)??"null");if(!x||x.draft?.projectId!==projectId||!Array.isArray(x.stages))throw new Error();setDraft(bindChiefDirectory(x.draft));setStages([fixedStage(x.stages[0]?.timeoutSeconds??60)]);setDirty(true);setDraftMessage("已载入下一版草稿，主指挥目录保持项目绑定；请重新审阅后保存配置。");}catch{setDraftMessage("没有可载入的下一版草稿。");}}
  const [planningBudgetReview,setPlanningBudgetReview]=useState<SavedProjectSetup>();
  const [stages,setStages]=useState<AcceptanceStageDraft[]>([]);const [dirty,setDirty]=useState(false);
  const saved=data?.projectSetups?.find(s=>s.setup.projectId===projectId);
  const projectRoot=data?.workflows.find(w=>w.project.id===projectId)?.project.root??"";
  const chiefDirectory=saved?.setup.chiefDirectory||(projectRoot?`${projectRoot}-chief`:undefined);
  function bindChiefDirectory(value:ProjectSetupDraft):ProjectSetupDraft {
    return {...removeUnusedCodexDefault(value,data?readApiModelDefault(data.dataDirectory,"worker"):undefined),validatorId:checkerId,checkArgs:[],chiefDirectory,worktree:chiefDirectory?undefined:value.worktree};
  }
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
    const nextDraft:ProjectSetupDraft = s ? {chiefDirectory:s.chiefDirectory,managementSources:s.managementSources,currencyBudget:s.policy.currencyBudget,resourceKeys:s.resourceKeys,profiles:s.profiles?.map(restoreProfile),worktree:s.worktree?{integration:s.worktree.integration?{taskId:s.worktree.integration.taskId,artifactPaths:s.worktree.integration.artifactPaths,validatorId:s.worktree.integration.validatorId,args:s.worktree.integration.check.args,timeoutSeconds:s.worktree.integration.check.timeoutSeconds}:undefined,inheritDependencies:s.worktree.input==="committed-raw-blobs-with-accepted-dependencies"?true:undefined,baseCommit:s.worktree.baseCommit,poolRoot:s.worktree.poolRoot,gitExecutable:s.worktree.git.path}:undefined,workerAdapter:s.workerAdapter??"codex",apiWorker:s.apiWorker,desktopWorker:s.desktopWorker,ompRuntimeParent:s.worker?.ompRuntimeParent,supervision:s.policy.supervision,projectId,requirement:s.requirement,codexExecutable:s.worker?.executable.path??"",workerModel:s.desktopWorker?.modelLabel??s.apiWorker?.model??s.worker?.model??"",chiefModel:s.chiefModel,deputyModel:s.deputyModel,validatorId:s.validatorId,checkArgs:s.check.args,artifactPaths:s.artifactPaths,workerMaxRequests:s.policy.maxRequests,managementMaxRequests:s.managementMaxRequests,maxSeconds:s.policy.maxSeconds} : {...empty,projectId,chiefDirectory:(data?.workflows.find(w=>w.project.id===projectId)?.project.root??"")+"-chief",codexExecutable:"",validatorId:data?.validators[0]?.id??""};
    if (!s && data) {
      const chiefDefault = readApiModelDefault(data.dataDirectory,"chief");
      const deputyDefault = readApiModelDefault(data.dataDirectory,"deputy");
      const workerDefault = readApiModelDefault(data.dataDirectory,"worker");
      if (chiefDefault || deputyDefault) nextDraft.managementSources = {chief:chiefDefault?.target??{kind:"openAi"},deputy:deputyDefault?.target??{kind:"openAi"}};
      nextDraft.chiefModel = chiefDefault?.model??"";
      nextDraft.deputyModel = deputyDefault?.model??"";
      if (workerDefault) {
        const adapter = workerDefault.target.kind === "openAi" ? "openai" : endpointProvider(workerDefault.target.endpoint);
        const worker = newApiWorker(adapter);
        if (worker) {
          nextDraft.workerAdapter = adapter;
          nextDraft.apiWorker = {...worker,target:workerDefault.target,model:workerDefault.model};
          nextDraft.workerModel = workerDefault.model;
          nextDraft.codexExecutable = "";
        }
      }
    }
    const restored=storageEnabled&&data?loadAccessDraft(data,projectId,projectRoot,saved?.digest??null):{state:"empty" as const};
    const next=restored.state==="restored"?{...nextDraft,...restored.values}:{...nextDraft};
    const previousAccess=accessKey(next);
    Object.assign(nextDraft,bindChiefDirectory(nextDraft));
    Object.assign(next,bindChiefDirectory(next));
    next.supervision={...(next.supervision??defaultSupervision),maxAdvicePerAttempt:Math.max(1,next.supervision?.maxAdvicePerAttempt??1),deliveryEnabled:true};
    next.profiles = next.profiles?.map(profile=>({...profile,description:profile.description.trim()?profile.description:automaticTaskDescription,
      supervisionOverride:canRecordSupervision(profile.workerAdapter)?undefined:profile.supervisionOverride}));
    const checked=(restored.state==="restored"?restored.fieldsChecked:!!s)&&previousAccess===accessKey(next);
    const reviewed=checked?accessKey(next):null;
    setDraft(next);setReviewedAccess(reviewed);setHydratedContext(accessContext);
    lastAccessWrite.current=JSON.stringify([accessContext,accessContents(next),reviewed]);
    setDirty(accessKey(next)!==accessKey(nextDraft));setAccessSaveError(restored.state==="invalid");
    setAccessNotice(restored.state==="restored"?(checked?"已恢复自动保存的接入配置，格式检查已通过；可直接修改。":"已恢复自动保存的接入草稿，请检查后继续。"):
      restored.state==="invalid"?"本机接入草稿无法读取，已保留原记录；请检查当前配置并重新保存。":
      restored.state==="superseded"?"项目配置已更新，已载入最新配置；旧接入草稿不会覆盖新版本。":"");
    setStages([fixedStage(s?.check.timeoutSeconds??60)]);
  },[accessContext]);
  const profileResourceContext=JSON.stringify([accessContext,draft.profiles?.map(profile=>[profile.id,workerConfigurationIdentity(profile)])]);
  useEffect(()=>{
    if(hydratedContext!==accessContext)return;
    let current=true;
    setAutomaticResources({});
    const profiles=draft.profiles??[];
    void Promise.all(profiles.map(async profile=>[profile.id,await suggestedWorkerResource(profile)] as const)).then(entries=>{
      if(!current)return;
      const generated=Object.fromEntries(entries);
      setAutomaticResources(generated);
      setDraft(previous=>{
        const next=previous.profiles?.map(profile=>generated[profile.id]?withAutomaticResource(profile,generated[profile.id],previous.resourceKeys):profile);
        if(!next?.some((profile,index)=>profile!==previous.profiles?.[index]))return previous;
        setDirty(true);
        return {...previous,profiles:next};
      });
    }).catch(()=>{if(current)setAccessNotice("自动资源生成失败，请按资源命名规则填写后保存。");});
    return()=>{current=false;};
  },[profileResourceContext,hydratedContext]);
  function persistAccess(checked:boolean) {
    if(!storageEnabled||!data)return true;
    const ok=saveAccessDraft(data.dataDirectory,projectRoot,saved?.digest??null,draft,checked);
    setAccessSaveError(!ok);
    setAccessNotice(ok?(checked?"格式检查通过，接入配置已自动保存；可随时修改。":"接入草稿已自动保存，修改后请重新检查。"):
      "接入配置自动保存失败，输入仍保留在当前窗口；请重试保存后再关闭。");
    return ok;
  }
  const currentAccess=accessKey(draft);
  const currentContents=accessContents(draft);
  const accessReviewChanged=!!accessReview&&(accessReview.context!==accessContext||accessReview.key!==currentAccess);
  useEffect(()=>{
    if(!connections||!accessReview||!accessReviewDialog.current)return;
    const dialog=accessReviewDialog.current;
    const opener=document.activeElement;
    dialog.showModal();
    return()=>{
      dialog.close();
      if(opener instanceof HTMLElement&&opener.isConnected)opener.focus();
    };
  },[connections,accessReview]);
  useEffect(()=>{
    if(!storageEnabled||hydratedContext!==accessContext||draft.projectId!==projectId)return;
    const stamp=JSON.stringify([accessContext,currentContents,reviewedAccess]);
    if(stamp===lastAccessWrite.current)return;
    lastAccessWrite.current=stamp;
    persistAccess(currentAccess===reviewedAccess);
  },[storageEnabled,hydratedContext,accessContext,currentAccess,currentContents,reviewedAccess]);
  async function openCliWindow() {
    try { await invoke("open_cli_session_window", {projectId}); }
    catch (error) { setAccessNotice(`CLI 会话窗口尚未打开。${String(error)}`); }
  }
  function checkAccessFields() {
    if(busy||pending||retiredAdapter||accessCheckLock.current)return false;
    if(data && !hasOmpKey(data) && [draft,...(draft.profiles??[])].some(item=>item.workerAdapter==="omp")){setOmpKeyPrompt(true);return false;}
    if(data?.desktopAccess?.checking){setAccessNotice("桌面会话仍在核对，请等待绑定完成。");setAccessSaveError(true);return false;}
    if(draft.profiles?.some(profile=>!automaticResources[profile.id])){setAccessNotice("正在生成执行端资源，请稍后检查保存。");return false;}
    const invalid=Array.from(connectionFields.current?.querySelectorAll<HTMLInputElement|HTMLSelectElement|HTMLTextAreaElement>("input,select,textarea")??[])
      .find(field=>!field.closest("[data-connection-actions]")&&!field.matches(":disabled")&&!field.checkValidity());
    if(invalid){revealConnectionField(invalid);invalid.reportValidity();return false;}
    if(resourceError(draft.resourceKeys)||(draft.profiles??[]).some(p=>resourceError(p.resourceKeys,draft.resourceKeys))||supervisionConflict){
      setAccessNotice("请先修正接入配置中的资源或监督冲突，再检查保存。");setAccessSaveError(true);return false;
    }
    return true;
  }
  async function validateAccess() {
    if(!checkAccessFields())return false;
    accessCheckLock.current=true;setAccessChecking(true);
    try {
      if(!persistAccess(true)||!await flushAccessStorage())return false;
      lastAccessWrite.current=JSON.stringify([accessContext,currentContents,currentAccess]);
      setReviewedAccess(currentAccess);
      if ("__TAURI_INTERNALS__" in window && (localWorker(draft.workerAdapter) || draft.profiles?.some(p => localWorker(p.workerAdapter)))) await openCliWindow();
      return true;
    }finally{accessCheckLock.current=false;setAccessChecking(false);}
  }
  function reviewAccessContinue() {
    if(checkAccessFields())setAccessReview({context:accessContext,key:currentAccess});
  }
  async function confirmAccessContinue() {
    if(!accessReview||accessReviewChanged)return;
    if(await validateAccess()){
      setAccessReview(null);
      onContinue?.();
    }
  }
  function update<K extends keyof ProjectSetupDraft>(key:K,value:ProjectSetupDraft[K]) {setDraft(d=>({...d,[key]:value}));setDirty(true);}
  function changeProfiles(profiles:WorkerProfileDraft[]) {
    setDraft(previous=>({...previous,profiles:profiles.length?profiles:undefined}));setDirty(true);
  }
  function changeProfileModel(index:number,model:string) {
    try {
      const profiles=(draft.profiles??[]).map((profile,n)=>n===index?{...profile,workerModel:model,apiWorker:profile.apiWorker?{...profile.apiWorker,model}:undefined}:profile);
      const synchronized=synchronizeWorkerProfiles(profiles,index,draft.resourceKeys??[],{files:false,resources:false});
      changeProfiles(synchronized.profiles);
      if(synchronized.notices.length)setAllocationNotice(synchronized.notices.join("；"));
    }catch(error){setAllocationNotice(String(error));}
  }
  function changeStages(next:AcceptanceStageDraft[]) {setStages(next);setDirty(true);}
  function changeStage(index:number,patch:Partial<AcceptanceStageDraft>) {changeStages(stages.map((s,i)=>i===index?{...s,...patch}:s));}
  const totalSeconds=stages.reduce((total,stage)=>total+stage.timeoutSeconds,0);
  const decisions=data?.managementDecisions?.filter(d=>d.projectId===projectId)??[];
  const statistics=data?.managementHistory.projects.find(project=>project.projectId===projectId);
  const requestCount=statistics?.total??0;
  const pending=(statistics?.pending??0)>0;
  const workflow=data?.workflows.find(w=>w.project.id===projectId);
  const desktopAdapter=draft.workerAdapter==="qoder"||draft.workerAdapter==="workbuddy";
  const retiredAdapter=draft.workerAdapter==="workbuddy"||(draft.workerAdapter??"codex")==="codex"||(draft.profiles??[]).some(p=>p.workerAdapter==="workbuddy"||p.workerAdapter==="codex");
  const defaultAdapter = draft.workerAdapter ?? "codex";
  const defaultSupervisionConflict = supervisionConflictFor(defaultAdapter, draft.supervision);
  const supervisionConflict=defaultSupervisionConflict || (draft.profiles??[]).some(p=>supervisionConflictFor(p.workerAdapter,effectiveSupervision(p.supervisionOverride,draft.supervision)));
  const locked=!!workflow?.approval && !workflow.state.revisionRequested;
  function planningRequestIssue(): string | undefined {
    if (retiredAdapter) return "请在接入管理中选择可用的执行端后保存配置。";
    if (busy || pending) return "当前操作正在处理，请等待结果后继续。";
    if (locked) return "当前方案已批准，请先完成本轮任务或请求方案修订。";
    if (!saved || dirty) return "请先保存需求与执行配置，保存成功后再请求主指挥生成方案。";
    if (saved.setup.policy.currencyBudget) return "当前配置使用金额预算，请在下方审阅规划预算许可。";
    if (data && managementKeyMissing(data, saved.setup.managementSources?.chief))
      return "请在接入管理中保存所选主指挥的 Key，然后请求生成方案。";
    if (requestCount >= saved.setup.managementMaxRequests) return "主副指挥请求次数已达到当前配置上限。";
    return undefined;
  }
  const requestIssue = planningRequestIssue();
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
    if (adapter === "omp" && data && !hasOmpKey(data)) setOmpKeyPrompt(true);
    setDraft({...draft, workerAdapter:adapter, apiWorker:newApiWorker(adapter), desktopWorker:undefined,
      codexExecutable:"", workerModel:"",
      ompRuntimeParent:undefined, worktree:method==="desktop"?undefined:draft.worktree});
    setDirty(true);
  }
  const selectedWorkerMethod=workerMethod(draft.workerAdapter);
  const FormContainer = connections ? "div" : "form";
  const workerFields = data ? <section className={connections?"role-configuration-fields":"role-model-fields"} aria-label="执行端模型配置">{!connections&&<HelpHeading as="h3" topic="ProjectPlanning">执行端</HelpHeading>}
          <div className="planning-fields">
            <div><label htmlFor="planning-adapter">编程执行器</label><select id="planning-adapter" value={draft.workerAdapter??"codex"} onChange={e=>{setDraft(d=>({...d,workerAdapter:e.target.value as ProjectWorkerAdapter,worktree:["qoder","workbuddy"].includes(e.target.value)?undefined:d.worktree,apiWorker:newApiWorker(e.target.value as ProjectWorkerAdapter),desktopWorker:undefined,codexExecutable:"",workerModel:"",ompRuntimeParent:undefined}));setDirty(true);}}>{draft.workerAdapter==="codex"&&<option value="codex" disabled>Codex · 执行接入已移除</option>}
              {(!connections||selectedWorkerMethod==="api")&&<option value="omp">OMP · DeepSeek API</option>}
              {(!connections||selectedWorkerMethod==="desktop")&&<><option value="qoder">Qoder · 已有桌面会话</option>{draft.workerAdapter==="workbuddy"&&<option value="workbuddy" disabled>WorkBuddy · 支持已移除</option>}</>}
              {(!connections||selectedWorkerMethod==="key")&&apiWorkerOptions.map(([id,label])=><option key={id} value={id}>{label}</option>)}</select></div>
            {draft.workerAdapter==="omp"&&<div><label htmlFor="planning-executable">编程程序路径</label><input id="planning-executable" required value={draft.codexExecutable} onChange={e=>update("codexExecutable",e.target.value)} placeholder="OMP 可执行文件的绝对路径" /></div>}
            {!desktopAdapter&&draft.workerAdapter!=="codex"&&<div><label htmlFor="planning-worker">编程模型 ID</label><input id="planning-worker" required value={draft.workerModel} onChange={e=>{const model=e.target.value;setDraft(d=>({...d,workerModel:model,apiWorker:d.apiWorker?{...d.apiWorker,model}:undefined}));setDirty(true);}} /></div>}
          </div>
          {draft.workerAdapter==="workbuddy"&&<p role="alert">WorkBuddy 支持已移除，历史配置保留。请选择其他执行器后保存新版配置。</p>}
          {draft.workerAdapter==="codex"&&<p role="alert">Codex 执行接入已移除，历史配置保留。请选择其他执行器；主指挥的 Codex 会话不受影响。</p>}
          {draft.workerAdapter==="qoder"&&<DesktopWorkerFields isolationEnabled={!!draft.chiefDirectory} data={data} projectId={projectId} app="qoder" send={send} busy={busy} worker={draft.desktopWorker} onChange={worker=>{setDraft(d=>({...d,desktopWorker:worker,workerModel:worker?.modelLabel??""}));setDirty(true);}}/>}
          {draft.apiWorker&&<ApiWorkerFields compact={connections} worker={draft.apiWorker} onChange={api=>update("apiWorker",api)}/>}
          <NativeWorkerCost adapter={draft.workerAdapter??"codex"}/>
          {draft.workerAdapter==="omp"&&(draft.chiefDirectory?<p className="secondary">OMP 运行配置固定保存在该模型目录内，与其他执行模型隔离。</p>:<><label htmlFor="planning-omp-runtime">OMP 独立运行目录（可选）</label><input id="planning-omp-runtime" value={draft.ompRuntimeParent??""} onChange={e=>update("ompRuntimeParent",e.target.value||undefined)} placeholder="已存在、可写的专用目录绝对路径"/><p className="secondary">用户目录含其他 Agent 配置时，请选择独立位置。须与项目和工作台数据目录分开，祖先目录不能含隐含配置。留空沿用工作台数据目录；更改后需重新规划和批准，历史运行文件保留。</p></>)}
          {connections&&draft.workerAdapter==="omp"&&<OmpConnectionTest data={data} busy={busy} send={send}
            executable={draft.codexExecutable} model={draft.workerModel} runtimeParent={draft.chiefDirectory?undefined:draft.ompRuntimeParent}/>}
          <datalist id="planning-chief-models">{managementModelList(data,draft.managementSources?.chief).map(m=><option key={m} value={m}/>)}</datalist><datalist id="planning-deputy-models">{managementModelList(data,draft.managementSources?.deputy).map(m=><option key={m} value={m}/>)}</datalist>
          <WorktreeFields isolationEnabled={!!draft.chiefDirectory} validators={data.validators} value={draft.worktree??undefined} disabled={desktopAdapter||(draft.profiles??[]).some(p=>p.workerAdapter==="qoder"||p.workerAdapter==="workbuddy")} onChange={value=>update("worktree",value)}/>
          <ResourceFields id="planning-resources" value={draft.resourceKeys} onChange={value=>update("resourceKeys",value)}/>
          {mode==="all"&&<WorkerProfiles automaticResources={automaticResources} send={send} busy={busy} isolationEnabled={!!draft.chiefDirectory} supervision={draft.supervision} commonResources={draft.resourceKeys??[]} data={data} projectId={projectId} value={draft.profiles??[]} worktree={!!draft.worktree} onChange={changeProfiles}/>}
          </section> : null;
  return <section className="panel project-planning" aria-label={connections?"项目角色接入配置":"主指挥项目规划"}>
    {ompKeyPrompt&&data&&<OmpKeyDialog data={data} busy={busy} send={send} onClose={()=>setOmpKeyPrompt(false)}/>}
    <div className="section-heading"><HelpHeading as="h2" topic="ProjectPlanning">{connections?"凭证与连接检查":"向主指挥提交需求"}</HelpHeading><span className="badge">{connections?((accessSaveError||durable.error)?"保存需处理":durable.pending?"正在保存":accessReady?"接入格式已检查":"接入待检查"):saved?`配置版本 ${saved.setup.version}`:accessReady?"接入已保留 · 需求待保存":"接入待检查"}</span></div>
    {!data?.workflows.length ? <><p className="secondary">请先关联项目目录，再填写主指挥来源和会话标识；独立检查和盲评验收工具均固定关联，无需登记外部程序。</p>{onSetup&&<button className="secondary-button" disabled={busy} onClick={onSetup}>关联项目并配置主指挥</button>}{connections&&data&&<IntegrationHub data={data} busy={busy} send={send} configure={()=>onSetup?.()}/>}</> : <>
      <label htmlFor="planning-project">{connections?"接入项目":"规划项目"}</label><select id="planning-project" value={projectId} disabled={busy||pending||accessChecking} onChange={e=>setProjectId(e.target.value)}>{data.workflows.map(w=><option value={w.project.id} key={w.project.id}>{w.project.root}</option>)}</select>
      <p className="secondary">{connections?"第 1 步：选择主指挥、副指挥与执行端。填写内容会按项目自动保存，下次打开自动恢复，可随时修改。检查后再填写需求与计划。":"第 2 步：填写项目需求，保存后请求主指挥提出任务和验收方案。交付文件由方案列明，批准前统一审阅。"} </p>
      {mode==="planning"&&<ProjectAccessSummary draft={draft} accessReady={accessReady} disabled={busy||pending} onConnections={onConnections}/>}
      {connections&&accessNotice&&<p role={accessSaveError||durable.error?"alert":"status"}>{durable.error|| (durable.pending?"正在保存接入设置到本机…":accessNotice)}</p>}
      <FormContainer onSubmit={connections ? undefined : async e=>{e.preventDefault();if(busy||pending)return;if(connections){if(await validateAccess())onContinue?.();return;}if(mode==="planning"&&!accessReady){onConnections?.();return;}if(locked){keepNextDraft();return;}if(budgetError||retiredAdapter)return;if(resourceError(draft.resourceKeys)||(draft.profiles??[]).some(p=>resourceError(p.resourceKeys,draft.resourceKeys)))return;if(await send({type:"configureProject",draft:{...draft,resourceKeys:resourceNames(draft.resourceKeys).length?resourceNames(draft.resourceKeys):undefined,profiles:draft.profiles?.map(p=>({...p,description:automaticTaskDescription,artifactPaths:[],acceptance:{schemaVersion:1,stages:stages.map(s=>({...s,validatorId:checkerId,args:[],expectedAssertions:[]}))},resourceKeys:resourceNames(p.resourceKeys).length?resourceNames(p.resourceKeys):undefined})),projectId,artifactPaths:[],validatorId:stages[0]?.validatorId??"",checkArgs:stages[0]?.args??[],acceptance:{schemaVersion:1,stages:stages.map(s=>({...s,validatorId:checkerId,args:[],expectedAssertions:[]}))}}})){setDirty(false);localStorage.removeItem(draftKey);setDraftMessage("");}}}>
        <fieldset disabled={busy||pending||accessChecking}>
          <fieldset className="planning-step-fields" hidden={connections} disabled={connections}>
          <label htmlFor="planning-requirement">项目需求</label>
          <RequirementComposer contextKey={JSON.stringify([data?.dataDirectory,projectId,saved?.digest,draft])}
            disabled={busy||pending||accessChecking||connections} value={draft.requirement} onChange={text=>update("requirement",text)}
            onWorkflow={(requirement,supervision)=>{setDraft(d=>({...d,requirement,...(supervision?{supervision}:{})}));setDirty(true);}}/>
          </fieldset>
          <fieldset ref={connectionFields} className="planning-step-fields" hidden={mode==="planning"} disabled={mode==="planning"}>
          {connections ? <ProjectConnections key={projectId} data={data} busy={busy} send={send} draft={draft} onChange={next=>{setDraft(next);setDirty(true);}}
            chief={<ManagementSourceFields role="chief" embedded method={draft.managementSources?.chief.kind==="codexSession"?"desktop":"api"} draft={draft} onChange={next=>{setDraft(next);setDirty(true);}}/>}
            deputy={<ManagementSourceFields role="deputy" embedded method="api" draft={draft} onChange={next=>{setDraft(next);setDirty(true);}}/>}
            worker={workerFields} onChiefMethod={chooseChiefMethod} onDefaultWorker={chooseDefaultWorker} onProfileModel={changeProfileModel}
            profiles={method=><WorkerProfiles automaticResources={automaticResources} methodFilter={method} send={send} busy={busy} showConnections
              isolationEnabled={!!draft.chiefDirectory} supervision={draft.supervision} commonResources={draft.resourceKeys??[]}
              data={data} projectId={projectId} value={draft.profiles??[]} worktree={!!draft.worktree}
              onChange={changeProfiles}/>}/> : <>
          <div className="planning-fields">
            <ManagementSourceFields draft={draft} onChange={next=>{setDraft(next);setDirty(true);}}/>
          </div>
            {workerFields}
          </>}
          <details className="integration-group project-directories" open={connections?undefined:true}>
            <summary>角色目录与隔离设置</summary>
          <section className="panel" aria-label="角色与模型工作目录"><HelpHeading as="h3" topic="ProjectPlanning">角色与模型工作目录</HelpHeading>
            <label htmlFor="planning-chief-directory">主指挥独立目录</label><input id="planning-chief-directory" readOnly aria-describedby="planning-chief-directory-note" value={draft.chiefDirectory??""}/>
            <p id="planning-chief-directory-note" className="secondary">由工作台绑定，只读展示。新项目默认使用项目目录旁的 -chief 目录；已有绑定和恢复草稿均保持本项目目录。</p>
            <p>副指挥与执行端总目录：<code>{workflow?.project.root}</code></p>
            <p className="secondary">副指挥只读调取主指挥原需求与方案。执行模型首次接入时，在总目录的 executors 下建立独立子目录；最多五个，切回已有模型复用原目录。模型切换不能改变正在运行尝试的目录。</p>
            <p className="secondary">正常执行只能读写该模型的工作目录。不同执行端须负责不同源码文件；副指挥会收到目录与文件边界，冲突分工不能派发。评审只提供独立验收工具处理后的只读片段，不开放其他目录。</p>
            {saved&&!saved.setup.chiefDirectory&&<p role="status">当前已保存配置尚未绑定独立目录。工作台已为新版草稿生成目录，请审阅、保存新版配置并重新规划、批准；历史记录仍保留。</p>}
            {saved?.setup.workerDirectories&&<ul>{Object.values(saved.setup.workerDirectories).map(d=>d&&<li key={d.identity}><code>{d.relativePath}</code></li>)}</ul>}
          </section>
          </details>
          </fieldset>
          <fieldset className="planning-step-fields" hidden={connections} disabled={connections}>
          <CurrencyBudgetFields resetKey={projectId+":"+(saved?.digest??"")} resetValue={saved?.setup.policy.currencyBudget} draft={draft} value={draft.currencyBudget} onChange={value=>update("currencyBudget",value)} onInvalid={setBudgetError}/>
          <p className="secondary">任务分工和实际交付文件由主指挥在方案中列明，批准前统一审阅；执行过程在“项目持续执行”的模型工作记录中持续更新。</p>
          <div className="planning-fields">
            <div><label htmlFor="planning-worker-limit">编程请求上限</label><input id="planning-worker-limit" type="number" required min={1} max={1000} value={draft.workerMaxRequests} onChange={e=>update("workerMaxRequests",Number(e.target.value))}/></div>
            <div><label htmlFor="planning-management-limit">项目主副指挥累计请求上限</label><input id="planning-management-limit" type="number" required min={1} max={1000} value={draft.managementMaxRequests} onChange={e=>update("managementMaxRequests",Number(e.target.value))}/></div>
            <div><label htmlFor="planning-seconds">单次执行时限（秒）</label><input id="planning-seconds" type="number" required min={10} max={3600} value={draft.maxSeconds} onChange={e=>update("maxSeconds",Number(e.target.value))}/></div>
            <div><label htmlFor="stage-timeout-0">单项客观检查时限（秒）</label><input id="stage-timeout-0" type="number" required min={1} max={3600} value={stages[0]?.timeoutSeconds??60} onChange={e=>changeStage(0,{timeoutSeconds:Number(e.target.value)})}/></div>
          </div>
          {totalSeconds>draft.maxSeconds&&<p role="alert">检查时限不能超过执行总时限。</p>}
          <p className="secondary">请求次数上限不等于费用上限。主副指挥分别使用所选接入，API 费用由对应供应商计收。{desktopAdapter?"桌面执行沿用所选应用的账号和模型；保存配置不会向该聊天发送消息。":draft.apiWorker?"API 执行使用接入管理中对应供应商的本机加密 Key；保存配置不发出模型请求。":draft.workerAdapter==="omp"?"OMP 使用本机保存的 DeepSeek API Key；程序应位于项目目录之外。":draft.chiefDirectory?"编程执行使用该模型独立 Windows 账户的 Codex 登录。":"编程执行使用本机 Codex 登录。"}</p>
          {supervisionConflict&&<p role="alert">请先修正默认或附加执行配置中的监督冲突，再保存。</p>}
          </fieldset>
        </fieldset>
        <div className={`planning-actions${connections?" access-step-actions":""}`} aria-label={connections?"接入配置操作":"需求配置操作"}>
          {allocationNotice&&<p role="status">{allocationNotice}</p>}
          {connections&&<div className="planning-action-notes">
            <p className="secondary">下一步会检查必填项与格式，确认后保存并继续。</p>
          </div>}
          <div className="planning-action-row">
            <div className="planning-action-buttons">
              {!connections&&<button type="submit" className="secondary-button" disabled={busy||pending||accessChecking||retiredAdapter||!data.validators.some(v=>v.id===checkerId)||totalSeconds>draft.maxSeconds||supervisionConflict||!!budgetError||(mode==="planning"&&!accessReady)}>{locked?"保存为下一版草稿":"保存需求与执行配置"}</button>}
              <button className="secondary-button" type="button" disabled={busy||pending||accessChecking} onClick={loadNextDraft}>载入下一版草稿</button>
              {connections&&(localWorker(draft.workerAdapter)||draft.profiles?.some(p=>localWorker(p.workerAdapter)))&&<button type="button" className="secondary-button" disabled={busy||pending||accessChecking||!projectId} onClick={()=>void openCliWindow()}>打开项目进度与 CLI 会话</button>}
            </div>
            {connections&&<button type="button" className="primary" disabled={busy||pending||accessChecking||retiredAdapter||supervisionConflict} onClick={reviewAccessContinue}>下一步：设置需求与计划</button>}
          </div>
        </div>
      </FormContainer>
      {locked && <p className="secondary">已有批准的任务，仍可编辑并保存后续模型配置草稿。原尝试保持原模型与目录；任务收口后请求方案修订，再保存下一版配置、重新规划并批准。</p>}
      {draftMessage&&<p role="status">{draftMessage}</p>}
      {mode!=="planning"&&saved&&<WorkerAccountControls saved={saved} data={data} disabled={busy||pending||dirty} send={send}/>}
      {!connections&&<>
      <ChiefPlanRequest key={accessContext} data={data} saved={saved} projectId={projectId} issue={requestIssue} busy={busy} send={send}/>
      {saved?.setup.policy.currencyBudget && <section className="panel" aria-label="规划金额许可">
        <p>主指挥生成方案也会产生费用。请先审阅规划预算许可，后续执行仍须批准具体方案，同一项目的费用和预留不会清零。</p>
        {data.currencyBudgets?.some(a=>a.projectId===projectId&&a.planningSetupDigest===saved.digest&&a.approvalLinked)&&<p role="status">当前配置的规划预算许可已记录。</p>}
        <button type="button" className="secondary-button" disabled={retiredAdapter||busy||dirty||pending||locked} onClick={()=>setPlanningBudgetReview(saved)}>审阅规划预算</button>
      </section>}
      {decisions.filter(d=>d.purpose!=="plan").map(d=><div key={d.operationId} className="planning-status"><strong>{states[d.state]??d.state}</strong>{d.message&&<p>{d.message}</p>}{d.state==="unknown"&&<p>没有可确认的回复，系统不会自动再次发送。</p>}</div>)}
      </>}
    </>}
    {connections&&accessReview&&createPortal(<dialog ref={accessReviewDialog} className="panel modal access-continue-dialog" aria-labelledby="access-continue-title"
      onCancel={event=>{event.preventDefault();if(!accessChecking)setAccessReview(null);}}>
      <h2 id="access-continue-title">确认接入配置</h2>
      <p>必填项与格式检查通过。确认后保存当前配置，进入需求与计划。</p>
      <dl className="access-continue-models">
        <dt>主指挥</dt><dd>{draft.chiefModel||(draft.managementSources?.chief.kind==="codexSession"?"已绑定的 Codex 会话":"待选择模型")}</dd>
        <dt>副指挥</dt><dd>{draft.deputyModel}</dd>
        <dt>执行端</dt><dd>{[draft.workerModel,...(draft.profiles??[]).map(profile=>profile.workerModel)].join("、")}</dd>
      </dl>
      <p className="secondary">连接可用性请使用各接入卡片中的连接测试核实。</p>
      {accessReviewChanged&&<p role="alert">配置已变化，请返回重新检查。</p>}
      {(accessSaveError||durable.error)&&<p role="alert">{durable.error||accessNotice}</p>}
      <div className="access-continue-buttons">
        <button type="button" className="secondary-button" disabled={accessChecking} onClick={()=>setAccessReview(null)}>返回修改</button>
        <button type="button" className="primary" disabled={busy||pending||accessChecking||accessReviewChanged} onClick={()=>void confirmAccessContinue()}>{accessChecking?"正在保存…":"确认保存并继续"}</button>
      </div>
    </dialog>,document.body)}
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
