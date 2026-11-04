import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { useRef, useState } from "react";
import type { ApiGenerationEntry, ApiGenerationLookup, ApiGenerationPage, ApiGenerationRecord, ApiGenerationResolutionInput, ApiGenerationReview, Command, CountResolutionResult } from "./contracts";
import { decimalAmount, parseDecimal } from "./CurrencyBudgetFields";
type Query = (command: Command) => Promise<CountResolutionResult>;
const same = (a: unknown,b: unknown) => JSON.stringify(a) === JSON.stringify(b);
const outcome = (s: string) => ({reserved:"等待计费结果",settled:"已按报价计费",notSent:"确认未发送",unknown:"结果未知",unpriced:"无法按报价计费"}[s] ?? s);
const usd = (n: number) => "$" + decimalAmount(n,9);
const identity = (r: ApiGenerationReview,e: ApiGenerationEntry) => r.input.projectId === e.projectId && r.input.requestId === e.requestId && r.taskId === e.taskId && r.attemptId === e.attemptId && r.round === e.round;
function checkedLookup(found: ApiGenerationLookup | null | undefined, e: ApiGenerationEntry, digest: string) {
  if (!found || found.projectId !== e.projectId || found.requestId !== e.requestId || found.reviewDigest !== digest
    || (found.record && (!identity(found.record.review,e) || found.record.review.reviewDigest !== digest))) throw new Error("返回确认与原 API 请求不符；请保留原记录核对。");
  return found.record;
}
export function ApiGenerationPanel({query,busy=false}:{query:Query;busy?:boolean}) {
  const [page,setPage]=useState<ApiGenerationPage|null>(null),[selected,setSelected]=useState<ApiGenerationEntry|null>(null);
  const [record,setRecord]=useState<ApiGenerationRecord|null>(null),[pending,setPending]=useState(false),[error,setError]=useState("");const gate=useRef(false);
  async function read(next=false) {
    if(gate.current||busy||selected)return;gate.current=true;setPending(true);setError("");
    try {const offset=next?page?.nextOffset:0;if(offset===null||offset===undefined)return;
      const r=await query({type:"readApiGenerations",offset,revision:next?page!.revision:null});
      if(!r.apiGenerations||r.apiGenerations.offset!==offset||(next&&r.apiGenerations.revision!==page!.revision))throw new Error("API 生成清单回复不一致，请重新读取首页。");
      setPage(r.apiGenerations);setRecord(null);
    } catch(e){setError(String(e));}finally{gate.current=false;setPending(false);}
  }
  async function original(e:ApiGenerationEntry) {
    if(gate.current||busy||selected||!e.feeResolution)return;gate.current=true;setPending(true);setError("");setRecord(null);
    try {const saved=e.feeResolution;const r=await query({type:"readApiGenerationConfirmation",projectId:e.projectId,requestId:e.requestId,reviewDigest:saved.reviewDigest});
      const found=checkedLookup(r.apiGenerationConfirmation,e,saved.reviewDigest);
      if(!found||found.userEventDigest!==saved.userEventDigest||found.review.input.amountUsdNanos!==saved.amountUsdNanos)throw new Error("原 API 生成费用确认与清单不符，请重新读取。");
      setRecord(found);
    }catch(err){setError(String(err));}finally{gate.current=false;setPending(false);}
  }
  return <section className="workflow-plans" aria-label="API 生成费用核对"><HelpHeading as="h2" topic="ApiGenerationPanel">API 生成费用核对</HelpHeading>
    <p>仅核对生成费用。计数费用、执行结果和请求次数分别保留；本次核对不会重新发送任务或标记任务完成。</p>
    <button disabled={busy||pending||!!selected} onClick={()=>void read()}>读取 API 生成清单</button>
    {error&&<p role="alert"><ErrorText error={error}/></p>}
    {page&&<><p>共 {page.total} 条；当前从第 {page.offset+(page.total?1:0)} 条显示。</p>
      {page.entries.map(e=><article className="panel" key={e.projectId+":"+e.requestId}>
        <HelpHeading as="h3" topic="ApiGenerationPanel">任务 {e.taskId} · 第 {e.round} 轮</HelpHeading><p>项目：{e.projectId}；尝试：{e.attemptId}</p><p>原请求：{e.requestId}</p>
        <p>原生成结果：{outcome(e.originalOutcome)}；原费用预留：{usd(e.maximumUsdNanos)}</p>
        {e.responseId&&<p>原响应：{e.responseId}</p>}{e.pricingIssue&&<p>原计费问题：{e.pricingIssue}</p>}
        <p>{e.feeResolution?`已确认 ${usd(e.feeResolution.amountUsdNanos!)}，${e.feeResolution.state==="recorded"?"已记账":"待发布"}`:e.originalOutcome==="settled"?"已按原报价记账，无需本次核销":e.originalOutcome==="notSent"?"已确认未发送，无需本次核销":e.originalOutcome==="reserved"?"等待原计费结果":"生成费用尚未核销"}</p>
        {!e.feeResolution&&<button disabled={busy||pending||!!selected||!["unknown","unpriced"].includes(e.originalOutcome)} onClick={()=>{setSelected(e);setRecord(null);}}>核对此 API 生成费用</button>}
        {e.feeResolution&&<button disabled={busy||pending||!!selected} onClick={()=>void original(e)}>读取原 API 生成费用确认</button>}
      </article>)}
      {page.nextOffset!==null&&<button disabled={busy||pending||!!selected} onClick={()=>void read(true)}>下一页 API 生成</button>}
    </>}
    {record&&<section className="panel" aria-label="原 API 生成费用确认"><HelpHeading as="h3" topic="ApiGenerationPanel">原 API 生成费用确认</HelpHeading>
      <p>项目：{record.review.input.projectId}；任务：{record.review.taskId}；尝试：{record.review.attemptId}；轮次：{record.review.round}</p>
      <p>金额：{usd(record.review.input.amountUsdNanos)}；账单引用：{record.review.input.billingReference}</p><p>{record.review.input.explanation}</p>
      <p>{record.state==="recorded"?"已记入原账本":"用户确认已保存，待发布"}；原生成结果：{outcome(record.review.originalOutcome)}</p>
    </section>}
    {selected&&<ApiGenerationForm entry={selected} query={query} busy={busy||pending} close={()=>setSelected(null)}/>}
  </section>;
}
export function ApiGenerationForm({entry,query,busy,close}:{entry:ApiGenerationEntry;query:Query;busy:boolean;close:()=>void}) {
  const [amount,setAmount]=useState(""),[reference,setReference]=useState(""),[explanation,setExplanation]=useState("");
  const [review,setReview]=useState<ApiGenerationReview|null>(null),[submitted,setSubmitted]=useState<ApiGenerationReview|null>(null);
  const [pending,setPending]=useState(false),[error,setError]=useState(""),[status,setStatus]=useState("");const gate=useRef(false);
  function edit(change:()=>void){change();setReview(null);setError("");}
  function input():ApiGenerationResolutionInput {
    for(const text of [reference,explanation])if(!text.trim()||text.includes("\0")||new TextEncoder().encode(text).length>8000)throw new Error("请填写账单引用和核对说明，均不超过 8000 字节。");
    return {projectId:entry.projectId,requestId:entry.requestId,amountUsdNanos:parseDecimal(amount,9),billingReference:reference,explanation};
  }
  async function act(confirm:boolean){
    if(gate.current||busy||submitted)return;
    try{const prepared=input();if(confirm&&(!review||!same(review.input,prepared)))throw new Error("内容已变化，请重新审阅。");
      gate.current=true;setPending(true);setError("");setStatus("");if(confirm)setSubmitted(review);
      const r=await query(confirm?{type:"confirmApiGenerationResolution",input:prepared,reviewDigest:review!.reviewDigest}:{type:"reviewApiGenerationResolution",input:prepared});
      if(confirm){const found=checkedLookup(r.apiGenerationConfirmation,entry,review!.reviewDigest);
        if(!found||!same(found.review,review))throw new Error("确认结果未能核实，请只读查询原确认；未重发。");
        setStatus(found.state==="recorded"?"原 API 生成费用已记入账本，请刷新清单和项目金额。":"用户确认已保存，账本待发布，请保留原数据并重开核心核对。");
      }else{if(!r.apiGenerationReview||!same(r.apiGenerationReview.input,prepared)||!identity(r.apiGenerationReview,entry)||r.apiGenerationReview.maximumUsdNanos!==entry.maximumUsdNanos||r.apiGenerationReview.originalOutcome!==entry.originalOutcome||r.apiGenerationReview.responseId!==entry.responseId||r.apiGenerationReview.pricingIssue!==entry.pricingIssue)throw new Error("返回审阅与原 API 生成不符，请刷新清单。");setReview(r.apiGenerationReview);}
    }catch(e){setError(String(e));if(!confirm)setReview(null);}finally{gate.current=false;setPending(false);}
  }
  async function lookup(){
    if(gate.current||busy||!submitted)return;gate.current=true;setPending(true);setError("");setStatus("");
    try{const r=await query({type:"readApiGenerationConfirmation",projectId:entry.projectId,requestId:entry.requestId,reviewDigest:submitted.reviewDigest});
      const found=checkedLookup(r.apiGenerationConfirmation,entry,submitted.reviewDigest);
      if(found&&!same(found.review,submitted))throw new Error("查询结果与原审阅不符，请保留记录核查。");
      setStatus(!found?"尚未找到原确认；未自动重发。":found.state==="recorded"?"原 API 生成费用已记入账本，请刷新清单和项目金额。":"原确认已保存，账本待发布，请保留原数据并重开核心核对。");
    }catch(e){setError(String(e));}finally{gate.current=false;setPending(false);}
  }
  return <section className="panel" aria-label="核对 API 生成费用"><HelpHeading as="h3" topic="ApiGenerationPanel">核对任务 {entry.taskId} 第 {entry.round} 轮生成费用</HelpHeading>
    <fieldset disabled={busy||pending||!!submitted}>
      <label>实际 API 生成金额（USD）<input aria-label="实际 API 生成金额（USD）" value={amount} onChange={e=>edit(()=>setAmount(e.target.value))}/></label>
      <label>API 生成账单引用<input aria-label="API 生成账单引用" value={reference} onChange={e=>edit(()=>setReference(e.target.value))}/></label>
      <label>API 生成核对说明<textarea aria-label="API 生成核对说明" value={explanation} onChange={e=>edit(()=>setExplanation(e.target.value))}/></label>
      <button onClick={()=>void act(false)}>审阅 API 生成费用</button>
    </fieldset>
    {review&&!submitted&&<section aria-label="API 生成费用审阅"><p>项目：{review.input.projectId}；任务：{review.taskId}；尝试：{review.attemptId}；轮次：{review.round}</p>
      <p>原请求：{review.input.requestId}；原结果：{outcome(review.originalOutcome)}</p>
      {review.responseId&&<p>原响应：{review.responseId}</p>}{review.pricingIssue&&<p>原计费问题：{review.pricingIssue}</p>}
      <p>原生成预留：{usd(review.maximumUsdNanos)}；核销金额：{usd(review.input.amountUsdNanos)}</p><p>账单引用：{review.input.billingReference}</p><p>{review.input.explanation}</p>
      <p>仅释放这笔生成预留并计入确认金额。工作台未向供应商验证账单；计数费用与原执行状态保留，旧回复中的工具不会放行，不会重发或恢复任务。</p>
      <button disabled={busy||pending} onClick={()=>void act(true)}>确认本次 API 生成核销</button>
    </section>}
    {submitted&&<><p>已提交原审阅，结果不明时仅查询原确认。</p><button disabled={busy||pending} onClick={()=>void lookup()}>只读查询原 API 生成确认</button></>}
    {error&&<p role="alert"><ErrorText error={error}/></p>}{status&&<p role="status">{status}</p>}
    <button disabled={pending} onClick={close}>关闭 API 生成核对</button>
  </section>;
}
