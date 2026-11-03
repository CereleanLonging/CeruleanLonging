import { ErrorText } from "./ErrorText";
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Command, Snapshot, TeamTimingAction, TeamTimingView } from "./contracts";

const phases:Record<string,string>={prepared:"材料已绑定",planning:"规划中",review:"等待用户审阅",approved:"方案已批准",executing:"执行与验收中",finished:"计时已结束",clockLost:"时钟已中断，待核对",interrupted:"已按中断记录",error:"记录需核查"};
const duration=(n:number|null)=>n===null?"未知":`${(n/1000).toFixed(3)} 秒`;
export function TeamTimingPanel({data,busy,send,desktop=false}:{data:Snapshot;busy:boolean;send:(c:Command)=>Promise<boolean>;desktop?:boolean}){
  const [recoveryOpening,setRecoveryOpening]=useState(false);
  const [recoveryNotice,setRecoveryNotice]=useState("");
  const [recoveryError,setRecoveryError]=useState("");
  async function openRecovery(){
    if(recoveryOpening||!desktop)return;
    setRecoveryOpening(true);setRecoveryError("");
    try{const session=await invoke<{directory:string;pid:number}>("open_maintenance_window",{importRetention:true});
      setRecoveryNotice(`已启动独立导入处置窗口（进程 ${session.pid}）。请关闭连接此工作台的窗口，再在处置窗口停止核心并核对备份。维护副本：${session.directory}`);
    }catch(e){setRecoveryError(String(e));}finally{setRecoveryOpening(false);}
  }
  const [path,setPath]=useState("");const [sha,setSha]=useState("");
  const [confirm,setConfirm]=useState<{projectId:string;setupDigest:string}|null>(null);
  const views=data.teamTimelines??[];
  const imported=data.teamImport;
  const action=(action:TeamTimingAction)=>send({type:"teamTiming",action});
  const [sending,setSending]=useState(false);
  async function once(a:TeamTimingAction){if(sending)return;setSending(true);try{await action(a);}finally{setSending(false);}}
  const disabled=busy||sending||!!(imported&&imported.state!=="completed");
  const selected=confirm&&data.projectSetups.find(s=>s.setup.projectId===confirm.projectId&&s.digest===confirm.setupDigest);
  function card(v:TeamTimingView){return <article className="panel" key={v.projectId}>
    <h3>{v.projectId} · {phases[v.phase]??v.phase}</h3>
    <dl><dt>总耗时</dt><dd>{duration(v.elapsedMs)}</dd><dt>规划</dt><dd>{duration(v.planningMs)}</dd>
      <dt>等待批准</dt><dd>{duration(v.approvalWaitMs)}</dd><dt>批准后执行与收口</dt><dd>{duration(v.executionAndClosureMs)}</dd>
      <dt>人工参与</dt><dd>{duration(v.humanActiveMs)}{v.humanActive?"（正在记录）":""}</dd></dl>
    {v.error&&<p role="alert"><ErrorText error={v.error}/></p>}
    {v.accepted!==null&&<p>{v.accepted?"原任务已通过验收。":"此记录不代表任务通过验收。"}</p>}
    {v.phase==="clockLost"&&<p>重开后不能恢复原单调时钟。核对原任务结束后，可将这段记录收口为中断；耗时保留未知。</p>}
    <div className="card-footer">
      <button type="button" disabled={disabled||!v.clockLive||!!v.error} onClick={()=>{if(v.startDigest)void once({kind:"human",projectId:v.projectId,startDigest:v.startDigest,active:!v.humanActive});}}>{v.humanActive?"结束人工参与":"开始人工参与"}</button>
      <button type="button" disabled={disabled||v.phase!=="executing"||v.humanActive||!!v.error} onClick={()=>{if(v.startDigest)void once({kind:"finish",projectId:v.projectId,startDigest:v.startDigest});}}>核对原结果并结束计时</button>
      {v.phase==="clockLost"&&<button type="button" disabled={disabled||!v.startDigest||!!v.error} onClick={()=>{if(v.startDigest)void once({kind:"closeInterrupted",projectId:v.projectId,startDigest:v.startDigest});}}>按中断收口原记录</button>}
    </div>
    <details><summary>计时记录引用</summary><code>{v.startDigest??"尚无可确认的引用"}</code></details>
  </article>;}
  return <section className="panel" aria-label="团队效率验收">
    <h2>团队效率验收</h2>
    <p>使用固定任务材料，在原项目流程中记录规划、等待批准、执行与人工参与时间。人工参与由你主动开关，任务批准仍在原方案卡片完成。</p>
    <p className="secondary">这是当前计时记录的读数，包含检查及收口开销。对比效率仍需相同配置的单执行者样本和真实用量；计时成功不等于正式发布验收通过。</p>
    {imported&&<section className="panel" aria-label="团队材料导入状态">
      <h3>{({running:"材料正在导入",completed:"原材料已导入",failed:"原导入未完成",interrupted:"原导入需核查"} as Record<string,string>)[imported.state]??"原导入需核查"}</h3>
      <p>{imported.message??"原导入结果已保存，下一步可审阅计时规划。"}</p>
      {imported.state!=="completed"&&<p>请读取同一操作的结果，不重复导入。关闭窗口后核心继续工作；若核心中断，已有记录会保留。</p>}
      {["failed","interrupted"].includes(imported.state)&&<details><summary>保留失败现场并继续其他工作</summary>
        <p>当前实验保持只读。可在“实验工作台”中创建另一个独立实验；新实验需要新的材料和明确批准，原任务不会自动复制或执行。</p>
        <p>在独立处置窗口中，可正常停止此目录的核心、创建冷备份、审阅并保存保留决定。结果不确定时，读取原决定核查。</p>
        <button type="button" disabled={!desktop||busy||recoveryOpening} onClick={()=>void openRecovery()}>打开独立导入处置窗口</button>
        {recoveryNotice&&<p role="status" style={{overflowWrap:"anywhere"}}>{recoveryNotice}</p>}
        {recoveryError&&<p role="alert"><ErrorText error={recoveryError}/></p>}
        <p>保留决定不会修复或清空原目录，也不会解除只读限制。备份包含本机加密凭证，不包含外部项目；它用于同用户原位置核查，不是可分享的便携项目包。</p>
      </details>}
      <details><summary>原导入引用</summary><p>{imported.sourcePath}</p><code>{imported.sourceSha256}</code><p>{imported.requestId}</p></details>
    </section>}
    {!data.workflows.length&&!imported&&<form onSubmit={e=>{e.preventDefault();void once({kind:"import",path:path.trim(),sha256:sha.trim().toLowerCase()});}}>
      <fieldset disabled={disabled}><legend>导入已冻结的团队材料</legend>
        <label htmlFor="timing-material-path">团队材料请求文件</label><input id="timing-material-path" required value={path} onChange={e=>setPath(e.target.value)} placeholder="D:\\验收材料\\request.json"/>
        <label htmlFor="timing-material-sha">材料文件 SHA256</label><input id="timing-material-sha" required pattern="[a-fA-F0-9]{64}" value={sha} onChange={e=>setSha(e.target.value)}/>
        <p>当前工作台目录必须与材料一致。导入将登记隔离项目、验收工具和执行配置；开始规划需下一步单独确认。</p>
        <button className="secondary-button">导入材料与执行配置</button>
      </fieldset>
    </form>}
    {data.projectSetups.filter(s=>views.some(v=>v.projectId===s.setup.projectId&&v.phase==="prepared")).map(s=><div key={s.setup.projectId}>
      <button type="button" className="secondary-button" disabled={disabled} onClick={()=>setConfirm({projectId:s.setup.projectId,setupDigest:s.digest})}>审阅 {s.setup.projectId} 的计时规划</button>
    </div>)}
    {views.map(card)}
    {confirm&&<div className="modal-backdrop"><section className="modal panel" role="dialog" aria-modal="true" aria-label="确认计时规划">
      <h3>确认计时规划</h3>
      {selected?<><p>{selected.setup.requirement}</p><p>主指挥：{selected.setup.chiefModel}。确认后开始计时，并通过原主指挥接入请求一次方案，可能产生供应商费用。此操作不批准或执行编程任务。</p></>:<p role="alert">执行配置已变化，请返回重新审阅。</p>}
      <p>需先绑定固定材料；已有规划或计时记录时核心会拒绝重复请求。</p>
      <button type="button" className="secondary-button" onClick={()=>setConfirm(null)}>返回</button>
      <button type="button" className="primary" disabled={disabled||!selected||!views.some(v=>v.projectId===confirm.projectId&&v.phase==="prepared")} onClick={async()=>{
        if(!selected||sending)return;setSending(true);try{await action({kind:"begin",...confirm});setConfirm(null);}finally{setSending(false);}
      }}>开始计时并请求方案</button>
    </section></div>}
  </section>;
}
