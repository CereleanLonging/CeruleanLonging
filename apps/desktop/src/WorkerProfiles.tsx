import { SupervisionFields } from "./SupervisionFields";
import { ResourceFields } from "./ResourceFields";
import type { AcceptanceStageDraft, ProjectWorkerAdapter, Snapshot, WorkerProfile, WorkerProfileDraft, SupervisionPolicy } from "./contracts";
import { ApiWorkerFields, apiWorkerOptions, newApiWorker } from "./ApiWorkerFields";
import { DesktopWorkerFields } from "./DesktopWorkerFields";
import { NativeWorkerCost } from "./NativeWorkerCost";

function nextId(prefix:string, used:string[]):string {
  let value=1;
  while(used.includes(`${prefix}-${value}`))value++;
  return `${prefix}-${value}`;
}

export function restoreProfile(p: WorkerProfile): WorkerProfileDraft {
  return {supervisionOverride:p.supervisionOverride,resourceKeys:p.resourceKeys,id:p.id,description:p.description,workerAdapter:p.workerAdapter,codexExecutable:p.worker?.executable.path??"",workerModel:p.worker?.model??p.apiWorker?.model??p.desktopWorker?.modelLabel??"",ompRuntimeParent:p.worker?.ompRuntimeParent,apiWorker:p.apiWorker,desktopWorker:p.desktopWorker,artifactPaths:p.artifactPaths,acceptance:{schemaVersion:1,stages:p.acceptance.stages.map(s=>({id:s.check.id,name:s.check.name??s.check.id,validatorId:s.validatorId,args:s.check.args,timeoutSeconds:s.check.timeoutSeconds}))}};
}
export function WorkerProfiles({data,projectId,value,worktree,commonResources,supervision,onChange}: {
  supervision?:SupervisionPolicy|null;commonResources:string[];data:Snapshot;projectId:string;value:WorkerProfileDraft[];worktree:boolean;onChange:(profiles:WorkerProfileDraft[])=>void;
}) {
  function update(index:number,patch:Partial<WorkerProfileDraft>) {onChange(value.map((p,i)=>i===index?{...p,...patch}:p));}
  function stages(index:number,next:AcceptanceStageDraft[]) {update(index,{acceptance:{schemaVersion:1,stages:next}});}
  return <section className="panel" aria-label="附加执行配置">
    <h3>附加执行配置</h3>
    <p className="secondary">主指挥可按任务选择下方配置或上面的默认执行者。每个配置有独立的模型、交付范围和检查；同一供应商可添加多个模型。选择会写入方案，批准后不能由副指挥替换。共享项目请求上限；并行派发按已批准的工作目录、依赖和资源条件执行。</p>
    {value.map((p,index)=>{
      const prefix=`profile-${index}`;
      const desktop=p.workerAdapter==="qoder"||p.workerAdapter==="workbuddy";
      return <section className="panel" aria-label={`执行配置 ${index+1}`} key={index}>
        <label htmlFor={`${prefix}-id`}>配置 ID</label><input id={`${prefix}-id`} required value={p.id} onChange={e=>update(index,{id:e.target.value})}/>
        <label htmlFor={`${prefix}-description`}>适用任务与特长</label><input id={`${prefix}-description`} required maxLength={2000} value={p.description} onChange={e=>update(index,{description:e.target.value})}/>
        <label htmlFor={`${prefix}-adapter`}>执行接入</label><select id={`${prefix}-adapter`} value={p.workerAdapter} onChange={e=>{const adapter=e.target.value as ProjectWorkerAdapter;update(index,{workerAdapter:adapter,codexExecutable:"",workerModel:"",ompRuntimeParent:undefined,desktopWorker:undefined,apiWorker:newApiWorker(adapter)});}}>
          <option value="codex">Codex Agent</option><option value="omp">OMP Agent</option>{apiWorkerOptions.map(([id,label])=><option key={id} value={id}>{label}</option>)}<option disabled={worktree} value="qoder">Qoder 已有聊天</option><option disabled={worktree} value="workbuddy">WorkBuddy 已有聊天</option>
        </select>
        {!desktop&&!p.apiWorker&&<><label htmlFor={`${prefix}-exe`}>执行程序绝对路径</label><input id={`${prefix}-exe`} required value={p.codexExecutable} onChange={e=>update(index,{codexExecutable:e.target.value})}/></>}
        {!desktop&&<><label htmlFor={`${prefix}-model`}>模型 ID</label><input id={`${prefix}-model`} required value={p.workerModel} onChange={e=>update(index,{workerModel:e.target.value,apiWorker:p.apiWorker?{...p.apiWorker,model:e.target.value}:undefined})}/></>}
        {p.workerAdapter==="omp"&&<><label htmlFor={`${prefix}-runtime`}>OMP 独立运行目录（可选）</label><input id={`${prefix}-runtime`} value={p.ompRuntimeParent??""} onChange={e=>update(index,{ompRuntimeParent:e.target.value||undefined})}/></>}
        {p.apiWorker&&<ApiWorkerFields idPrefix={`${prefix}-api`} worker={p.apiWorker} onChange={apiWorker=>update(index,{apiWorker})}/>}
        <NativeWorkerCost adapter={p.workerAdapter}/>
        {desktop&&<DesktopWorkerFields idPrefix={`${prefix}-desktop`} data={data} projectId={projectId} app={p.workerAdapter as 'qoder'|'workbuddy'} worker={p.desktopWorker} onChange={desktopWorker=>update(index,{desktopWorker,workerModel:desktopWorker?.modelLabel??""})}/>}
        <SupervisionFields id={`${prefix}-supervision`} value={p.supervisionOverride} inherited={supervision} adapter={p.workerAdapter} onChange={supervisionOverride=>update(index,{supervisionOverride})}/>
        <ResourceFields id={`${prefix}-resources`} common={commonResources} value={p.resourceKeys} onChange={resourceKeys=>update(index,{resourceKeys})}/>
        <label htmlFor={`${prefix}-artifacts`}>任务交付文件（每行一个）</label><textarea id={`${prefix}-artifacts`} required value={p.artifactPaths.join("\n")} onChange={e=>update(index,{artifactPaths:e.target.value.split(/\r?\n/)})}/>
        {p.acceptance.stages.map((stage,n)=>{
          const key=`${prefix}-check-${n}`;
          function patch(change:Partial<AcceptanceStageDraft>) {stages(index,p.acceptance.stages.map((s,i)=>i===n?{...s,...change}:s));}
          return <section className="panel" key={stage.id} aria-label={`检查 ${n+1}`}>
            <label htmlFor={`${key}-name`}>检查名称</label><input id={`${key}-name`} required value={stage.name} onChange={e=>patch({name:e.target.value})}/>
            <label htmlFor={`${key}-validator`}>已登记验收工具</label><select id={`${key}-validator`} required value={stage.validatorId} onChange={e=>patch({validatorId:e.target.value})}><option value="">选择工具</option>{data.validators.map(v=><option key={v.id} value={v.id}>{v.name}</option>)}</select>
            <label htmlFor={`${key}-args`}>检查参数（每行一个）</label><textarea id={`${key}-args`} value={stage.args.join("\n")} onChange={e=>patch({args:e.target.value?e.target.value.split(/\r?\n/):[]})}/>
            <label htmlFor={`${key}-seconds`}>检查时限（秒）</label><input id={`${key}-seconds`} required type="number" min={1} max={3600} value={stage.timeoutSeconds} onChange={e=>patch({timeoutSeconds:Number(e.target.value)})}/>
            <button type="button" className="secondary-button" disabled={p.acceptance.stages.length===1} onClick={()=>stages(index,p.acceptance.stages.filter((_,i)=>i!==n))}>删除本项检查</button>
          </section>;
        })}
        <div className="card-footer"><button type="button" className="secondary-button" disabled={p.acceptance.stages.length>=8} onClick={()=>stages(index,[...p.acceptance.stages,{id:nextId("check",p.acceptance.stages.map(s=>s.id)),name:"任务检查",validatorId:data.validators[0]?.id??"",args:[],timeoutSeconds:30}])}>增加检查</button><button type="button" className="secondary-button" onClick={()=>onChange(value.filter((_,i)=>i!==index))}>移除执行配置</button></div>
      </section>;
    })}
    <button type="button" className="secondary-button" disabled={value.length>=16} onClick={()=>onChange([...value,{id:nextId("profile",value.map(p=>p.id)),description:"",workerAdapter:"deepseek",supervisionOverride:{mode:"disabled"},codexExecutable:"",workerModel:"",apiWorker:newApiWorker("deepseek"),artifactPaths:[],acceptance:{schemaVersion:1,stages:[{id:"profile-check",name:"任务检查",validatorId:data.validators[0]?.id??"",args:[],timeoutSeconds:30}]}}])}>添加执行配置</button>
  </section>;
}
