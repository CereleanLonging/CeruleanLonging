import { ProjectApiConnections } from "./ProjectConnections";
import { OmpConnectionTest } from "./OmpCredentials";
import { HelpHeading } from "./HelpHeading";
import { SupervisionFields } from "./SupervisionFields";
import { ResourceFields } from "./ResourceFields";
import type { Command, ProjectWorkerAdapter, Snapshot, WorkerProfile, WorkerProfileDraft, SupervisionPolicy } from "./contracts";
import { ApiWorkerFields, apiWorkerOptions, newApiWorker } from "./ApiWorkerFields";
import { DesktopWorkerFields } from "./DesktopWorkerFields";
import { NativeWorkerCost } from "./NativeWorkerCost";
import { defaultWorkerAdapter, workerMethod, workerMethodLabels, type WorkerMethod } from "./ConnectionMethods";
import { useState } from "react";
import { automaticTaskDescription, synchronizeWorkerProfiles } from "./workerConfiguration";

function nextId(prefix:string, used:string[]):string {
  let value=1;
  while(used.includes(`${prefix}-${value}`))value++;
  return `${prefix}-${value}`;
}

export function restoreProfile(p: WorkerProfile): WorkerProfileDraft {
  return {supervisionOverride:p.supervisionOverride,resourceKeys:p.resourceKeys,id:p.id,description:p.description,workerAdapter:p.workerAdapter,codexExecutable:p.worker?.executable.path??"",workerModel:p.worker?.model??p.apiWorker?.model??p.desktopWorker?.modelLabel??"",ompRuntimeParent:p.worker?.ompRuntimeParent,apiWorker:p.apiWorker,desktopWorker:p.desktopWorker,artifactPaths:p.artifactPaths,acceptance:{schemaVersion:1,stages:p.acceptance.stages.map(s=>({expectedAssertions:s.check.expectedAssertions,id:s.check.id,name:s.check.name??s.check.id,validatorId:s.validatorId,args:s.check.args,timeoutSeconds:s.check.timeoutSeconds}))}};
}
export function WorkerProfiles({data,projectId,value,worktree,commonResources,supervision,onChange,isolationEnabled=false,send,busy=false,showConnections=false,methodFilter,automaticResources={}}: {
  automaticResources?:Record<string,string>;
  methodFilter?:WorkerMethod;send?:(command:Command)=>Promise<boolean>;busy?:boolean;showConnections?:boolean;isolationEnabled?:boolean;supervision?:SupervisionPolicy|null;commonResources:string[];data:Snapshot;projectId:string;value:WorkerProfileDraft[];worktree:boolean;onChange:(profiles:WorkerProfileDraft[],editedIndex?:number)=>void;
}) {
  const [syncNotices,setSyncNotices]=useState<string[]>([]);
  const connectionsOnly=showConnections&&!!methodFilter;
  function update(index:number,patch:Partial<WorkerProfileDraft>,syncFiles=false) {
    try {
      const synchronized=synchronizeWorkerProfiles(value.map((p,i)=>i===index?{...p,...patch}:p),index,commonResources,{files:false,resources:Object.hasOwn(patch,"resourceKeys")});
      setSyncNotices(synchronized.notices);onChange(synchronized.profiles,syncFiles?index:undefined);return true;
    }catch(error){setSyncNotices([String(error)]);return false;}
  }
  const addAdapter=methodFilter?defaultWorkerAdapter[methodFilter]:"deepseek";
  return <section className={methodFilter?"filtered-worker-profiles":"panel"} aria-label={methodFilter?`${workerMethodLabels[methodFilter]}附加配置`:"附加执行配置"}>
    {syncNotices.map(notice=><p role="status" key={notice}>{notice}。</p>)}
    {!methodFilter&&<><HelpHeading as="h3" topic="WorkerProfiles">附加执行配置</HelpHeading>
    <p className="secondary">主指挥可按任务选择下方配置或上面的默认执行者。每个配置保留模型接入；任务分工与交付文件在方案中统一批准。同一供应商可添加多个模型。选择会写入方案，批准后不能由副指挥替换。共享项目请求上限；并行派发按已批准的工作目录、依赖和资源条件执行。</p></>}
    {value.map((p,index)=>{
      if(methodFilter&&workerMethod(p.workerAdapter)!==methodFilter)return null;
      const prefix=`profile-${index}`;
      const desktop=p.workerAdapter==="qoder"||p.workerAdapter==="workbuddy";
      return <section className="panel" aria-label={`执行配置 ${index+1}`} key={index} onBlur={event=>{
        if(event.target.id===`${prefix}-id`&&value.some((other,n)=>n!==index&&other.id===p.id)){
          const id=nextId(p.id.slice(0,64),value.map(profile=>profile.id));update(index,{id});setSyncNotices([`配置 ID 已占用，当前配置已改为 ${id}`]);return;
        }
        if([`${prefix}-model`,`${prefix}-api-workspace`,`${prefix}-api-region`,`${prefix}-api-relay-origin`,`${prefix}-adapter`].includes(event.target.id))update(index,{},true);
      }}>
        <div className="section-heading"><strong>{p.id} · {p.workerModel||"待选择模型"}</strong></div>
        {<><label htmlFor={`${prefix}-id`}>配置 ID</label><input id={`${prefix}-id`} required maxLength={80} pattern="[A-Za-z0-9_-]+" value={p.id} onChange={e=>update(index,{id:e.target.value})}/></>}
        {<>
        <label htmlFor={`${prefix}-adapter`}>执行接入</label><select id={`${prefix}-adapter`} value={p.workerAdapter} onChange={e=>{const adapter=e.target.value as ProjectWorkerAdapter;update(index,{workerAdapter:adapter,codexExecutable:"",workerModel:"",ompRuntimeParent:undefined,desktopWorker:undefined,apiWorker:newApiWorker(adapter)});}}>
          {p.workerAdapter==="codex"&&<option value="codex" disabled>Codex · 执行接入已移除</option>}
          {(!methodFilter||methodFilter==="cli"||methodFilter==="api")&&<option value="omp">OMP · DeepSeek API</option>}
          {(!methodFilter||methodFilter==="cli"||methodFilter==="key")&&apiWorkerOptions.map(([id,label])=><option key={id} value={id}>{label}</option>)}
          {(!methodFilter||methodFilter==="cli"||methodFilter==="desktop")&&<><option disabled={worktree} value="qoder">Qoder 已有聊天</option>{p.workerAdapter==="workbuddy"&&<option disabled value="workbuddy">WorkBuddy · 支持已移除</option>}</>}
        </select>
        {p.workerAdapter==="omp"&&<><label htmlFor={`${prefix}-exe`}>执行程序绝对路径</label><input id={`${prefix}-exe`} required value={p.codexExecutable} onChange={e=>update(index,{codexExecutable:e.target.value})}/></>}
        {!desktop&&p.workerAdapter!=="codex"&&<><label htmlFor={`${prefix}-model`}>模型 ID</label><input id={`${prefix}-model`} required value={p.workerModel} onChange={e=>update(index,{workerModel:e.target.value,apiWorker:p.apiWorker?{...p.apiWorker,model:e.target.value}:undefined})}/></>}
        {p.workerAdapter==="omp"&&(isolationEnabled?<p className="secondary">OMP 运行配置固定保存在该模型目录内，与其他执行模型隔离。</p>:<><label htmlFor={`${prefix}-runtime`}>OMP 独立运行目录（可选）</label><input id={`${prefix}-runtime`} value={p.ompRuntimeParent??""} onChange={e=>update(index,{ompRuntimeParent:e.target.value||undefined})}/></>)}
        {p.apiWorker&&<ApiWorkerFields compact={connectionsOnly} idPrefix={`${prefix}-api`} worker={p.apiWorker} onChange={apiWorker=>update(index,{apiWorker})}/>}
        <NativeWorkerCost adapter={p.workerAdapter}/>
        {showConnections&&send&&p.workerAdapter==="omp"&&<OmpConnectionTest data={data} busy={busy} send={send}
          executable={p.codexExecutable} model={p.workerModel} runtimeParent={isolationEnabled?undefined:p.ompRuntimeParent}/>}
        {p.workerAdapter==="workbuddy"&&<p role="alert">WorkBuddy 支持已移除，请替换此执行器或删除该附加配置。</p>}
        {p.workerAdapter==="qoder"&&<DesktopWorkerFields send={send} busy={busy} isolationEnabled={isolationEnabled} selectionKey={p.id} idPrefix={`${prefix}-desktop`} data={data} projectId={projectId} app="qoder" worker={p.desktopWorker} onChange={desktopWorker=>update(index,{desktopWorker,workerModel:desktopWorker?.modelLabel??""})}/>}
        {showConnections&&!methodFilter&&send&&p.apiWorker&&<ProjectApiConnections target={p.apiWorker.target} prefix={prefix} data={data} busy={busy} send={send} view="key"
          modelAssignments={[{id:p.id,label:`附加执行配置 ${p.id}`,target:p.apiWorker.target,model:p.workerModel,
            onSelect:model=>update(index,{workerModel:model,apiWorker:p.apiWorker?{...p.apiWorker,model}:undefined},true)}]}/>}
        </>}
        {!connectionsOnly&&<><SupervisionFields id={`${prefix}-supervision`} value={p.supervisionOverride} inherited={supervision} adapter={p.workerAdapter} onChange={supervisionOverride=>update(index,{supervisionOverride})}/>
        <ResourceFields id={`${prefix}-resources`} common={commonResources} value={p.resourceKeys} automatic={automaticResources[p.id]} onChange={resourceKeys=>update(index,{resourceKeys})}/>
        </>}
        {<div className="card-footer"><button type="button" className="secondary-button" onClick={()=>onChange(value.filter((_,i)=>i!==index))}>移除执行配置</button></div>}
      </section>;
    })}
    {methodFilter!=="cli"&&<button type="button" className="secondary-button" disabled={busy||value.length>=16||(methodFilter==="desktop"&&worktree)} onClick={()=>onChange([...value,{id:nextId("profile",value.map(p=>p.id)),description:automaticTaskDescription,workerAdapter:addAdapter,codexExecutable:"",workerModel:"",apiWorker:newApiWorker(addAdapter),artifactPaths:[],acceptance:{schemaVersion:1,stages:[{id:"profile-check",name:"任务检查",validatorId:data.validators[0]?.id??"",args:[],timeoutSeconds:30}]}}])}>{methodFilter?`添加${workerMethodLabels[methodFilter]}执行配置`:"添加执行配置"}</button>}
  </section>;
}
