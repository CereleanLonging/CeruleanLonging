import {useEffect,useLayoutEffect,useRef,useState} from "react";
import {HelpHeading} from "./HelpHeading";
import {ErrorText} from "./ErrorText";
import type {EventAnchor,Snapshot,WorkspaceActivityEntry,WorkspaceActivityPage} from "./contracts";
import type {WorkspaceReader} from "./WorkspaceBrowser";

const stateNames:Record<string,string>={starting:"正在启动",executing:"工作中",submitted:"已提交结果",unknown:"结果待核对",retired:"已结束",abandoned:"已关闭",cancelled:"已取消",failed:"执行失败",reconciling:"核对中","cancel-requested":"请求停止中",cancelling:"停止中",running:"请求进行中",ready:"回复待处理",applied:"回复已处理",applying:"回复入账中",awaitingExternal:"等待绑定会话回复",discarded:"已放弃",startupClosed:"启动已关闭"};
const actor=(value:string)=>value==="chief"?"主指挥":value==="deputy"?"副指挥":value==="user"?"用户":value==="core"?"工作台核心":value.startsWith("worker")?"执行端":value;
function liveRows(data:Snapshot, projectId:string){
 const managers=(data.managementDecisions??[]).filter(d=>d.projectId===projectId).map(d=>{const chief=["plan","accept","refine"].includes(d.purpose);return {id:d.operationId,model:"",role:chief?"主指挥":"副指挥",task:"",state:stateNames[d.state]??"等待处理",message:d.message??""};});
 const workers=[...(data.apiJobs??[]),...(data.ompJobs??[]),...(data.codexJobs??[])].filter(j=>j.projectId===projectId).map(j=>({id:j.attemptId,model:j.worker.model,role:"执行端",task:j.taskId,state:stateNames[j.state]??"等待处理",message:j.message??""}));
 const desktops=(data.desktopAccess?.tasks??[]).filter(t=>t.review.projectId===projectId).map(t=>({id:t.review.attempt.id,model:t.review.observation.modelLabel??"",role:"桌面执行端",task:t.review.attempt.taskId,state:t.submitted?"已提交结果":t.retired?"已结束":t.stopping?"停止中":t.active?"工作中":"等待核对",message:t.progress?.latest?.statusText??t.error??""}));
 return [...managers,...workers,...desktops].slice(-20);
}
export function ProjectActivity({data,read,enabled=true}:{data:Snapshot|null;read:WorkspaceReader;enabled?:boolean}){
 const [selected,setSelected]=useState("");
 const projects=data?.workflows??[];
 const projectId=projects.some(p=>p.project.id===selected)?selected:projects[0]?.project.id??"";
 return <section className="panel project-activity" aria-label="模型工作记录">
  <div className="section-heading"><HelpHeading as="h3" topic="ProjectRuns">模型工作记录</HelpHeading><span className="badge">只读 · 持续同步</span></div>
  <label htmlFor="activity-project">查看项目</label><select id="activity-project" value={projectId} disabled={!enabled||!projects.length} onChange={e=>setSelected(e.target.value)}>{!projects.length&&<option value="">尚未关联项目</option>}{projects.map(p=><option key={p.project.id} value={p.project.id}>{p.project.root.split(/[\\/]/).at(-1)} · {p.project.id}</option>)}</select>
  {projectId&&data?<ActivityStream key={projectId} projectId={projectId} data={data} read={read} enabled={enabled}/>:<div className="activity-empty">关联项目后，这里持续显示主指挥、副指挥和执行模型的工作记录。</div>}
 </section>;
}
function ActivityStream({projectId,data,read,enabled}:{projectId:string;data:Snapshot;read:WorkspaceReader;enabled:boolean}){
 const [entries,setEntries]=useState<WorkspaceActivityEntry[]>([]),[total,setTotal]=useState(0),[error,setError]=useState(""),[paused,setPaused]=useState(false),[follow,setFollow]=useState(true),[filter,setFilter]=useState(""),[reading,setReading]=useState(false),[lastAt,setLastAt]=useState("");
 const [hasOlder,setHasOlder]=useState(false),[catchingUp,setCatchingUp]=useState(false);
 const scrollRestore=useRef<{height:number;top:number}|null>(null);
 const list=useRef<HTMLDivElement>(null),reader=useRef(read),cursor=useRef<EventAnchor|null>(null),loaded=useRef<WorkspaceActivityEntry[]>([]),alive=useRef(false),locked=useRef(false),requestGeneration=useRef(0),followRef=useRef(follow);
 reader.current=read;followRef.current=follow;
 async function load(older=false,reset=false){
  if(locked.current||!enabled)return;
  locked.current=true;setReading(true);const generation=requestGeneration.current;
  const before=older?loaded.current[0]?.anchor??null:null;
  const anchor=reset?null:cursor.current;
  try{
   const reply=await reader.current({type:"projectActivity",projectId,after:older?null:anchor,before});
   if(!alive.current||generation!==requestGeneration.current)return;
   if(reply.type!=="projectActivity"||reply.value.projectId!==projectId)throw Error("项目记录身份不符");
   const value:WorkspaceActivityPage=reply.value;
   for(let i=0;i<value.entries.length;i++){
    const item=value.entries[i];
    if(!Number.isSafeInteger(item.anchor.sequence)||!item.anchor.hash||i&&item.anchor.sequence<=value.entries[i-1].anchor.sequence)throw Error("记录顺序无效");
    if(!older&&anchor&&item.anchor.sequence<=anchor.sequence)throw Error("收到重复或倒退的记录游标");
   }
   const merged=new Map((reset?[]:loaded.current).map(e=>[e.anchor.sequence,e]));
   for(const item of value.entries){const previous=merged.get(item.anchor.sequence);if(previous&&previous.anchor.hash!==item.anchor.hash)throw Error("原记录摘要变化，请保留现有内容核对");merged.set(item.anchor.sequence,item);}
   let next=[...merged.values()].sort((a,b)=>a.anchor.sequence-b.anchor.sequence);
   if(next.length>400)next=older?next.slice(0,400):next.slice(-400);
   if(!older)cursor.current=value.entries.at(-1)?.anchor??anchor??value.head;
   if(older&&list.current)scrollRestore.current={height:list.current.scrollHeight,top:list.current.scrollTop};
   if(older||reset||!anchor)setHasOlder(value.hasOlder);else if(next[0]?.anchor.sequence!==loaded.current[0]?.anchor.sequence)setHasOlder(true);
   if(!older)setCatchingUp(value.hasNewer);
   loaded.current=next;setEntries(next);setTotal(value.total);setError("");setLastAt(new Date().toLocaleTimeString());
  }catch(reason){if(alive.current&&generation===requestGeneration.current)setError(String(reason));}
  finally{if(alive.current&&generation===requestGeneration.current){locked.current=false;setReading(false);}}
 }
 useEffect(()=>{alive.current=true;let timer:ReturnType<typeof setTimeout>;
  async function poll(){if(!paused&&followRef.current&&document.visibilityState!=="hidden")await load();if(alive.current)timer=setTimeout(()=>void poll(),2000);}
  void poll();return()=>{alive.current=false;requestGeneration.current++;locked.current=false;clearTimeout(timer);};
 },[projectId,enabled,paused]);
 useLayoutEffect(()=>{const el=list.current;if(!el)return;if(scrollRestore.current){el.scrollTop=scrollRestore.current.top+el.scrollHeight-scrollRestore.current.height;scrollRestore.current=null;}else if(followRef.current)el.scrollTop=el.scrollHeight;},[entries]);
 const live=liveRows(data,projectId);
 const options=[...new Set(entries.map(e=>e.model||actor(e.principal)).concat(live.map(e=>e.model||e.role)))];
 const visible=entries.filter(e=>!filter||(e.model||actor(e.principal))===filter);
 return <>
  <p className="secondary">按工作台实际收到的事件与状态更新，记录保存在本机；这是工作进度流，不是模型逐字回复或内部推理。未收到记录时不会推测完成，盲评票据在揭晓前保持密封。</p>
  <div className="activity-toolbar"><label>模型 / 角色<select aria-label="筛选模型或角色" value={filter} onChange={e=>setFilter(e.target.value)}><option value="">全部模型与角色</option>{options.map(o=><option key={o}>{o}</option>)}</select></label>
   <button type="button" className="secondary-button" onClick={()=>setPaused(p=>!p)}>{paused?"恢复记录刷新":"暂停记录刷新"}</button>
   <button type="button" className="secondary-button" disabled={reading} onClick={()=>{followRef.current=true;setFollow(true);void load(false,true);}}>重新读取记录</button>
   <button type="button" className="secondary-button" disabled={reading} onClick={()=>{followRef.current=true;setFollow(true);void load(false,true);}}>跟随最新</button>
  </div>
  <p role="status" className="secondary">{paused?"记录刷新已暂停，任务继续执行":follow?"每 2 秒同步已落盘记录":"正在阅读历史，点击“跟随最新”继续同步"}{catchingUp&&" · 正在补读积压记录"}{lastAt&&` · 上次同步 ${lastAt}`}</p>
  {error&&<p role="alert"><ErrorText error={error}/>；已保留上次内容。</p>}
  {!!live.length&&<details className="activity-live" open><summary>接入模型当前状态</summary><div>{live.filter(r=>!filter||r.model===filter||r.role===filter).map(r=><article key={`${r.role}-${r.id}`}><strong>{r.role} · {r.model||"原记录未注明模型"}</strong><span className="badge">{r.state}</span>{r.task&&<p>任务 {r.task}</p>}{r.message&&<p>{r.message}</p>}</article>)}</div></details>}
  {!filter&&data.workerObservation?.projectId===projectId&&<details className="activity-live"><summary>已收到的执行工具状态 · 任务 {data.workerObservation.taskId}</summary><p className="secondary">仅展示核心已记录的命令或文件变更状态，不展开原始工具参数。</p><ul>{data.workerObservation.trace.events.slice(-20).map(e=><li key={e.sequence}>#{e.sequence} · {e.kind==="command"?"命令执行":"文件变更"} · {e.status==="completed"?"完成":e.status==="failed"?"失败":"已拒绝"}{e.exitCode!==null&&` · 退出码 ${e.exitCode}`}</li>)}</ul></details>}
  {!filter&&data.ompFiles?.projectId===projectId&&<details className="activity-live"><summary>已收到的 OMP 文件记录 · 任务 {data.ompFiles.taskId}</summary><ul>{data.ompFiles.files.slice(-20).map(f=><li key={f.callId}><code>{f.path??"路径未记录"}</code> · {f.state}</li>)}</ul></details>}
  <div ref={list} className="activity-stream" role="log" aria-label="持续工作记录" aria-live="off" tabIndex={0} onScroll={e=>{const el=e.currentTarget;if(el.scrollHeight-el.scrollTop-el.clientHeight>=40){followRef.current=false;setFollow(false);}}}>
   {hasOlder&&<button type="button" disabled={reading} onClick={()=>{followRef.current=false;setFollow(false);void load(true);}}>读取更早记录</button>}
   {!visible.length&&<p className="activity-empty">{entries.length?"当前筛选没有记录。":"尚无已落盘的工作事件。模型接入不会自动启动任务。"}</p>}
   {visible.map(item=><article className="activity-entry" key={item.anchor.hash}><div><span className="activity-sequence">#{item.anchor.sequence}</span><strong>{item.title}</strong><time dateTime={item.at}>{new Date(item.at).toLocaleString()}</time></div><p className="secondary">{actor(item.principal)}{item.model&&` · ${item.model}`}{item.taskId&&` · 任务 ${item.taskId}`}</p><p className="activity-detail">{item.detail}</p>{!!item.files.length&&<details><summary>交付文件摘要</summary><ul>{item.files.map(file=><li key={file}><code>{file}</code></li>)}</ul></details>}</article>)}
  </div>
  <p className="secondary">当前载入 {entries.length} / {total} 条已落盘事件，窗口最多保留 400 条；更早记录可回读。暂停刷新只影响这个窗口。</p>
 </>;
}
