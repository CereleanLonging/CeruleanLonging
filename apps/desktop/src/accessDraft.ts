import type { DesktopWorkerSelection, ProjectSetupDraft, Snapshot, WorkerProfileDraft } from "./contracts";
import { setAccessItem } from "./accessStorage";

export function accessSelection(draft: ProjectSetupDraft) {
  const { chiefDirectory, managementSources, chiefModel, deputyModel, workerAdapter, workerModel,
    codexExecutable, apiWorker, desktopWorker, ompRuntimeParent, profiles, worktree, resourceKeys } = draft;
  return {chiefDirectory,managementSources,chiefModel,deputyModel,workerAdapter,workerModel,
    codexExecutable,apiWorker,desktopWorker,ompRuntimeParent,profiles,worktree,resourceKeys};
}
function stable(value: unknown): string {
  return JSON.stringify(value, (_key, item) => item && typeof item === "object" && !Array.isArray(item)
    ? Object.fromEntries(Object.entries(item).sort(([a],[b]) => a.localeCompare(b))) : item);
}
export const accessKey = (draft: ProjectSetupDraft) => stable(accessSelection(draft));
type DesktopReference = {bindingId:string;projectRoot:string;modelLabel:string};
type StoredProfile = Omit<WorkerProfileDraft,"desktopWorker"> & {desktopWorker?:DesktopReference | null};
type StoredSelection = Omit<ReturnType<typeof accessSelection>,"desktopWorker"|"profiles"> & {desktopWorker?:DesktopReference | null;profiles?:StoredProfile[]};
type RecordV1 = {schema:1;projectId:string;projectRoot:string;basedOn:string|null;savedAt:string;fieldsChecked:boolean;selection:StoredSelection};
export const accessDraftKey = (directory:string, projectId:string) => JSON.stringify(["cerulean-access-draft",1,directory,projectId]);
const desktopReference = (worker:DesktopWorkerSelection|null|undefined):DesktopReference|undefined => worker
  ? {bindingId:worker.binding.id,projectRoot:worker.projectRoot,modelLabel:worker.modelLabel} : undefined;

// Strict shape checks keep corrupt/old local drafts out of the form. Only these
// non-credential configuration fields may be persisted; unknown keys are rejected.
type Check = (value:unknown)=>boolean;
const str:Check = v=>typeof v==="string";
const num:Check = v=>typeof v==="number"&&Number.isFinite(v);
const bool:Check = v=>typeof v==="boolean";
const optional=(check:Check):Check=>v=>v==null||check(v);
const list=(check:Check):Check=>v=>Array.isArray(v)&&v.every(check);
const oneOf=(...values:unknown[]):Check=>v=>values.includes(v);
const union=(...checks:Check[]):Check=>v=>checks.some(check=>check(v));
const shape=(fields:Record<string,Check>):Check=>v=>!!v&&typeof v==="object"&&!Array.isArray(v)
  &&Object.keys(v).every(key=>Object.hasOwn(fields,key))&&Object.entries(fields).every(([key,check])=>check((v as Record<string,unknown>)[key]));
const endpoint=union(shape({kind:oneOf("deepSeek","anthropic","kimiChina","grok","a6Api","a6ApiVip","qwenSingapore","qwenHongKong")}),shape({kind:oneOf("qwenBeijing"),workspaceId:str}));
const apiTarget=union(shape({kind:oneOf("openAi")}),shape({kind:oneOf("provider"),endpoint}));
const sources=shape({chief:union(apiTarget,shape({kind:oneOf("codexSession"),sessionId:str})),deputy:union(apiTarget,shape({kind:oneOf("deepSeek")}))});
const apiWorker=shape({target:apiTarget,model:str,fileTools:list(oneOf("read-files","write-files","run-commands","network","desktop-send")),maxRounds:num,maxToolCalls:num,maxOutputTokens:num,maxTotalTokens:num,requestTimeoutSeconds:num,writeScope:optional(oneOf("declared-artifacts"))});
const desktop=shape({bindingId:str,projectRoot:str,modelLabel:str});
const stage=shape({id:str,name:str,validatorId:str,args:list(str),timeoutSeconds:num,expectedAssertions:optional(list(str))});
const acceptance=shape({schemaVersion:oneOf(1),stages:list(stage)});
const supervision=union(shape({mode:oneOf("disabled")}),shape({mode:oneOf("enabled"),policy:shape({schemaVersion:oneOf(1),repeatedFailureThreshold:num,maxAdvicePerAttempt:num,deliveryEnabled:optional(bool)})}));
const adapter=oneOf("codex","omp","qoder","workbuddy","openai","anthropic","deepseek","qwen","kimi","grok","a6api");
const workerFields={workerAdapter:optional(adapter),workerModel:str,codexExecutable:str,apiWorker:optional(apiWorker),desktopWorker:optional(desktop),ompRuntimeParent:optional(str),resourceKeys:optional(list(str))};
const profile=shape({...workerFields,id:str,description:str,workerAdapter:adapter,artifactPaths:list(str),acceptance,supervisionOverride:optional(supervision)});
const worktree=shape({baseCommit:str,poolRoot:str,gitExecutable:str,inheritDependencies:optional(bool),integration:optional(shape({taskId:str,artifactPaths:list(str),validatorId:str,args:list(str),timeoutSeconds:num}))});
const selection=shape({...workerFields,chiefDirectory:optional(str),managementSources:optional(sources),chiefModel:str,deputyModel:str,profiles:optional(list(profile)),worktree:optional(worktree)});
const record=shape({schema:oneOf(1),projectId:str,projectRoot:str,basedOn:optional(str),savedAt:str,fieldsChecked:bool,selection});
const maxBytes=256*1024;

export function saveAccessDraft(directory:string, projectRoot:string, basedOn:string|null, draft:ProjectSetupDraft, fieldsChecked:boolean):boolean {
  const values=accessSelection(draft);
  const value:RecordV1={schema:1,projectId:draft.projectId,projectRoot,basedOn,savedAt:new Date().toISOString(),fieldsChecked,
    selection:{...values,desktopWorker:desktopReference(values.desktopWorker),profiles:values.profiles?.map(p=>({...p,desktopWorker:desktopReference(p.desktopWorker)}))}};
  try {
    const encoded=JSON.stringify(value);
    if(encoded.length>maxBytes||!record(JSON.parse(encoded)))return false;
    return setAccessItem(accessDraftKey(directory,draft.projectId),encoded);
  } catch {return false;}
}
export function loadAccessDraft(data:Snapshot, projectId:string, projectRoot:string, basedOn:string|null):
  {state:"empty"|"invalid"|"superseded"}|{state:"restored";values:ReturnType<typeof accessSelection>;fieldsChecked:boolean} {
  try {
    const encoded=localStorage.getItem(accessDraftKey(data.dataDirectory,projectId));
    if(encoded===null)return {state:"empty"};
    if(encoded.length>maxBytes)return {state:"invalid"};
    const raw:unknown=JSON.parse(encoded);
    if(!record(raw))return {state:"invalid"};
    const value=raw as RecordV1;
    if(value.projectId!==projectId||value.projectRoot!==projectRoot||value.basedOn!==basedOn)return {state:"superseded"};
    let desktopReview=false;
    const restoreDesktop=(ref:DesktopReference|null|undefined):DesktopWorkerSelection|undefined=>{
      if(!ref)return;
      desktopReview=true;
      const binding=data.desktopAccess?.bindings.find(b=>b.id===ref.bindingId&&b.projectId===projectId&&b.observation.modelLabel===ref.modelLabel&&b.bridgeSha256===data.desktopAccess?.bridgeSha256);
      // A persisted choice is not a fresh observation or a renewed permission.
      return binding?{binding,projectRoot:ref.projectRoot,modelLabel:ref.modelLabel,workspaceConfirmed:false,externalToolsAcknowledged:false}:undefined;
    };
    const values={...value.selection,desktopWorker:restoreDesktop(value.selection.desktopWorker),profiles:value.selection.profiles?.map(p=>({...p,desktopWorker:restoreDesktop(p.desktopWorker)}))};
    return {state:"restored",values,fieldsChecked:value.fieldsChecked&&!desktopReview};
  } catch {return {state:"invalid"};}
}
