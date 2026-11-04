import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import {useRef,useState} from "react";
import type {BaselineAction,BaselineReview,Command,Snapshot} from "./contracts";
import {ExecutionAuthorization} from "./ExecutionAuthorization";

const phases:Record<string,string>={imported:"材料已导入",needsRead:"等待核对原记录",preparePending:"准备方案中",review:"等待用户批准",approvePending:"正在核对批准",approved:"已批准，尚未执行",startPending:"执行与验收中",returned:"原执行已返回，等待结束计时",finishPending:"正在核对收口",abortPending:"正在停止计时",closePending:"正在记录中断",readPending:"只读核对中",closed:"已收口",clockLost:"原时钟已丢失",unknown:"结果未知",error:"需要核查"};
const duration=(n:number|null|undefined)=>n==null?"未知":`${(n/1000).toFixed(3)} 秒`;
export function BaselinePanel({data,busy,send}:{data:Snapshot;busy:boolean;send:(c:Command)=>Promise<boolean>}){
  const [id,setId]=useState("");const [path,setPath]=useState("");const [sha,setSha]=useState("");
  const pending=useRef(false);const [sending,setSending]=useState(false);
  const [review,setReview]=useState<{value:BaselineReview;mode:"approve"|"start"}|null>(null);
  const views=data.baselines??[];const disabled=busy||sending;const working=views.some(v=>v.busy);
  async function once(action:BaselineAction){if(pending.current)return false;pending.current=true;setSending(true);try{return await send({type:"baseline",action});}finally{pending.current=false;setSending(false);}}
  const selected=review&&views.find(v=>v.id===review.value.id&&v.specDigest===review.value.specDigest&&v.timing?.startDigest===review.value.startDigest);
  const current=!!selected&&!selected.busy&&!selected.error&&!selected.cancelRequested&&!selected.timing?.humanActive&&selected.timing?.clockLive&&selected.phase===(review?.mode==="approve"?"review":"approved");
  return <section className="panel" aria-label="单执行者效率验收">
    <HelpHeading as="h2" topic="BaselinePanel">单执行者效率验收</HelpHeading>
    <p>使用一名执行者和原独立验收流程，不请求主、副指挥。准备方案只读取本地固定材料；批准与实际执行分开确认。</p>
    <p>实验记录保存到当前工作台的独立子目录，导入材料中的数据目录会改为该位置。项目、材料和 Git 工作目录保持原选择，须与工作台及既有实验分离。导入后暂不支持在此工作台新增或重配其他项目。</p>
    <form onSubmit={e=>{e.preventDefault();void once({kind:"import",id:id.trim(),path:path.trim(),sha256:sha.trim().toLowerCase()});}}>
      <fieldset disabled={disabled||working}><HelpHeading as="legend" topic="BaselinePanel">导入固定基线材料</HelpHeading>
        <label htmlFor="baseline-id">实验名称</label><input id="baseline-id" required pattern="[A-Za-z0-9][A-Za-z0-9_-]{0,63}" value={id} onChange={e=>setId(e.target.value)}/>
        <label htmlFor="baseline-path">基线材料请求文件</label><input id="baseline-path" required value={path} onChange={e=>setPath(e.target.value)}/>
        <label htmlFor="baseline-sha">基线材料 SHA256</label><input id="baseline-sha" required pattern="[a-fA-F0-9]{64}" value={sha} onChange={e=>setSha(e.target.value)}/>
        <button>导入基线材料</button>
      </fieldset>
    </form>
    {views.map(v=>{const t=v.timing;const ref={id:v.id,startDigest:t?.startDigest??""};const frozen=data.baselineReview?.id===v.id?data.baselineReview:null;
      return <article className="panel" key={v.id}><HelpHeading as="h3" topic="BaselinePanel">{v.id} · {phases[v.phase]??v.phase}</HelpHeading>
        {v.error&&<p role="alert"><ErrorText error={v.error}/></p>}
        {v.cancelRequested&&<p>已保存停止请求。正在运行的操作会核对原结果；这不代表供应商已确认取消。</p>}
        <dl><dt>总耗时</dt><dd>{duration(t?.elapsedMs)}</dd><dt>准备方案</dt><dd>{duration(t?.planningMs)}</dd><dt>等待批准</dt><dd>{duration(t?.approvalWaitMs)}</dd><dt>批准后执行与收口</dt><dd>{duration(t?.executionAndClosureMs)}</dd><dt>人工参与</dt><dd>{duration(t?.humanActiveMs)}{t?.humanActive?"（正在记录）":""}</dd></dl>
        {t?.accepted!=null&&<p>{t.accepted?"原任务独立验收通过。":"此记录不代表任务验收通过。"}</p>}
        {v.phase==="clockLost"&&<p>原计时无法恢复，耗时保留未知。只核对原任务，不重新执行。</p>}
        <div className="card-footer">
          <button type="button" disabled={disabled||working||!v.bindingDigest} onClick={()=>{if(v.bindingDigest)void once({kind:"read",id:v.id,bindingDigest:v.bindingDigest});}}>核对并查看原方案</button>
          {v.phase==="imported"&&<button type="button" disabled={disabled||working||v.cancelRequested||!v.bindingDigest} onClick={()=>{if(v.bindingDigest)void once({kind:"prepare",id:v.id,bindingDigest:v.bindingDigest});}}>开始计时并准备方案</button>}
          {v.phase==="review"&&<button type="button" disabled={disabled||working||!frozen||!!v.error||v.cancelRequested||!!t?.humanActive} onClick={()=>{if(frozen)setReview({value:structuredClone(frozen),mode:"approve"});}}>审阅单执行者方案</button>}
          {v.phase==="approved"&&<button type="button" disabled={disabled||working||!frozen||!!v.error||v.cancelRequested||!!t?.humanActive} onClick={()=>{if(frozen)setReview({value:structuredClone(frozen),mode:"start"});}}>审阅单次执行</button>}
          <button type="button" disabled={disabled||!t?.clockLive||!t.startDigest||["approvePending","finishPending","abortPending"].includes(v.phase)} onClick={()=>void once({kind:"human",...ref,active:!t?.humanActive})}>{t?.humanActive?"结束人工参与":"开始人工参与"}</button>
          <button type="button" disabled={disabled||!t?.clockLive||v.cancelRequested||!["preparePending","review","approvePending","approved","startPending"].includes(v.phase)} onClick={()=>void once({kind:"cancel",...ref})}>请求停止原任务</button>
          {v.phase==="returned"&&<button type="button" disabled={disabled||working||!!t?.humanActive} onClick={()=>void once({kind:"finish",...ref})}>核对结果并结束计时</button>}
          {t?.clockLive&&["planning","review","approved"].includes(t.phase)&&!v.busy&&<button type="button" disabled={disabled||working||t.humanActive} onClick={()=>void once({kind:"abortBeforeExecution",...ref})}>停止本次实验并保留记录</button>}
          {v.phase==="clockLost"&&<button type="button" disabled={disabled||working||!t?.startDigest} onClick={()=>void once({kind:"closeInterrupted",...ref})}>按中断收口原记录</button>}
        </div>
      </article>;
    })}
    {review&&<div className="modal-backdrop"><section className="modal panel" role="dialog" aria-modal="true" aria-label="单执行者方案确认">
      <HelpHeading as="h3" topic="BaselinePanel">{review.mode==="approve"?"批准原固定方案":"确认单次执行"}</HelpHeading>
      <ExecutionAuthorization revision={review.value.plan}/>
      <p>实际执行使用当前工作台中对应供应商的凭证，可能产生费用。批准不会自动开始执行；执行失败或未知不自动重发。计时包含材料检查、验收与收口开销，不等于纯模型推理耗时。</p>
      {!current&&<p role="alert">原方案或状态已变化，请返回重新核对。</p>}
      <button type="button" onClick={()=>setReview(null)}>返回</button>
      <button type="button" disabled={disabled||working||!current} onClick={async()=>{if(!current)return;const ok=await once({kind:review.mode,id:review.value.id,startDigest:review.value.startDigest,specDigest:review.value.specDigest});if(ok)setReview(null);}}>{review.mode==="approve"?"批准此方案（不执行）":"使用已配置凭证执行一次"}</button>
    </section></div>}
  </section>;
}
