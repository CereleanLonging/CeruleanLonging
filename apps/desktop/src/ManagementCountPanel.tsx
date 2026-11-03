import { ErrorText } from "./ErrorText";
import { useRef, useState } from "react";
import type { Command, CountResolutionInput, CountResolutionRecord, CountResolutionResult, CountResolutionReview, ManagementCountEntry, ManagementCountPage } from "./contracts";
import { decimalAmount, parseDecimal } from "./CurrencyBudgetFields";

type Query = (command: Command) => Promise<CountResolutionResult>;
const outcome = (value: string) => ({pending: "等待结果", counted: "已取得收据", unknown: "结果未知"}[value] ?? value);
export function ManagementCountPanel({ query, busy = false }: { query: Query; busy?: boolean }) {
  const [page, setPage] = useState<ManagementCountPage | null>(null);
  const [selected, setSelected] = useState<ManagementCountEntry | null>(null);
  const [error, setError] = useState(""); const [pending, setPending] = useState(false); const gate = useRef(false);
  const [record, setRecord] = useState<CountResolutionRecord | null>(null);
  async function readOriginal(entry: ManagementCountEntry, aspect: "settleFee" | "retireContinuation") {
    const saved = aspect === "settleFee" ? entry.feeResolution : entry.retirement;
    if (!saved || gate.current || busy) return;
    gate.current = true; setPending(true); setError(""); setRecord(null);
    try {
      const result = await query({type: "readManagementCountConfirmation", projectId: entry.projectId, operationId: entry.operationId, aspect, reviewDigest: saved.reviewDigest});
      const found = result.confirmation;
      if (!found?.record || found.projectId !== entry.projectId || found.operationId !== entry.operationId || found.aspect !== aspect || found.reviewDigest !== saved.reviewDigest
        || found.record.review.reviewDigest !== saved.reviewDigest || found.record.userEventDigest !== saved.userEventDigest
        || found.record.review.input.projectId !== entry.projectId || found.record.review.input.operationId !== entry.operationId || found.record.review.input.action.kind !== aspect) throw new Error("原计数确认与清单不符，请刷新并核对。");
      setRecord(found.record);
    } catch(e) { setError(String(e)); } finally { gate.current = false; setPending(false); }
  }
  async function read(next = false) {
    if (gate.current || busy) return;
    gate.current = true; setPending(true); setError("");
    try {
      const offset = next ? page?.nextOffset : 0;
      if (offset === null || offset === undefined) return;
      const result = await query({ type: "readManagementCounts", offset, revision: next ? page!.revision : null });
      if (!result.counts || result.counts.offset !== offset || (next && result.counts.revision !== page!.revision)) throw new Error("计数清单回复不一致，请重新读取首页。");
      setPage(result.counts);
    } catch(e) { setError(String(e)); } finally { gate.current = false; setPending(false); }
  }
  return <section className="workflow-plans" aria-label="计数费用与续发核对"><h2>计数费用与续发核对</h2>
    <p>费用核销与终止续发分别确认。核销只调整原计数费用；终止保留费用、原结果和请求次数，不会重新发送。</p>
    <button disabled={busy || pending} onClick={() => void read()}>读取计数清单</button>
    {error && <p role="alert"><ErrorText error={error}/></p>}
    {page && <><p>共 {page.total} 条；当前从第 {page.offset + (page.total ? 1 : 0)} 条显示。</p>
      {page.entries.map(e => <article className="panel" key={e.projectId + ":" + e.operationId}><h3>{e.operationId}</h3>
        <p>项目：{e.projectId}；原计数结果：{outcome(e.originalOutcome)}；原费用预留：${decimalAmount(e.maximumUsdNanos,9)}</p>
        <p>计数费：{e.feeResolution ? `已确认${e.feeResolution.state === "recorded" ? "并记账" : "，待发布"} $${decimalAmount(e.feeResolution.amountUsdNanos ?? 0,9)}` : "尚未核销"}；续发：{e.retirement ? (e.retirement.state === "recorded" ? "已终止" : "终止待发布") : "尚未人工终止"}</p>
        <button disabled={busy || pending || !!selected || e.originalOutcome === "pending" || (!!e.feeResolution && !!e.retirement)} onClick={() => setSelected(e)}>核对原操作</button>
        {e.feeResolution && <button disabled={busy || pending} onClick={() => void readOriginal(e,"settleFee")}>读取原费用确认</button>}
        {e.retirement && <button disabled={busy || pending} onClick={() => void readOriginal(e,"retireContinuation")}>读取原终止确认</button>}
      </article>)}
      {page.nextOffset !== null && <button disabled={busy || pending} onClick={() => void read(true)}>下一页计数</button>}
    </>}
    {record && <section className="panel" aria-label="原计数确认"><h3>{record.review.input.operationId} 原确认</h3>
      {record.review.input.action.kind === "settleFee" ? <><p>金额：${decimalAmount(record.review.input.action.amountUsdNanos,9)}；账单引用：{record.review.input.action.billingReference}</p><p>{record.review.input.action.explanation}</p></> : <p>终止原因：{record.review.input.action.reason}</p>}
      <p>{record.state === "recorded" ? "已记入账本" : "用户确认已保存，账本待发布"}；原计数结果：{outcome(record.review.originalOutcome)}</p>
      <details><summary>原确认引用</summary><p>用户确认：<code>{record.userEventDigest}</code></p><p>审阅：<code>{record.review.reviewDigest}</code></p><p>计数来源：<code>{record.review.sourceDigest}</code></p></details>
    </section>}
    {selected && <CountResolutionForm key={selected.projectId + selected.operationId} entry={selected} query={query} busy={busy || pending} close={() => setSelected(null)}/>}
  </section>;
}

export function CountResolutionForm({ entry, query, busy, close }: { entry: ManagementCountEntry; query: Query; busy: boolean; close: () => void }) {
  const [kind, setKind] = useState<"settleFee" | "retireContinuation">(entry.feeResolution ? "retireContinuation" : "settleFee");
  const [amount, setAmount] = useState(""); const [reference, setReference] = useState(""); const [explanation, setExplanation] = useState("");
  const [review, setReview] = useState<CountResolutionReview | null>(null); const [submitted, setSubmitted] = useState<CountResolutionReview | null>(null);
  const [pending, setPending] = useState(false); const [error, setError] = useState(""); const [status, setStatus] = useState(""); const gate = useRef(false);
  const same = (a: unknown,b: unknown) => JSON.stringify(a) === JSON.stringify(b);
  function edit(action: () => void) { action(); setReview(null); setError(""); }
  function input(): CountResolutionInput {
    if (!explanation.trim() || (kind === "settleFee" && !reference.trim())) throw new Error("请填写明确的核对说明；费用核销还需要账单引用。");
    return { projectId: entry.projectId, operationId: entry.operationId, action: kind === "settleFee" ? { kind, amountUsdNanos: parseDecimal(amount,9), billingReference: reference, explanation } : { kind, reason: explanation } };
  }
  async function act(confirm: boolean) {
    if (gate.current || busy || submitted) return;
    try {
      const prepared = input();
      if (confirm && (!review || !same(review.input,prepared))) throw new Error("内容已变化，请重新审阅。");
      gate.current = true; setPending(true); setError(""); setStatus("");
      if (confirm) setSubmitted(review);
      const result = await query(confirm ? { type: "confirmManagementCountResolution", input: prepared, reviewDigest: review!.reviewDigest } : { type: "reviewManagementCountResolution", input: prepared });
      if (confirm) {
        const found = result.confirmation;
        if (!found || found.projectId !== prepared.projectId || found.operationId !== prepared.operationId || found.reviewDigest !== review!.reviewDigest || !found.record || !same(found.record.review,review)) throw new Error("确认结果未能核实，请只读查询原确认；未重发。");
        setStatus(found.record.state === "recorded" ? "原确认已记入账本。请刷新计数清单。" : "原确认已保存，账本待发布，请保留数据并重开核心核对。");
      } else {
        if (!result.review || !same(result.review.input,prepared)) throw new Error("返回审阅与输入不符。");
        setReview(result.review);
      }
    } catch(e) { setError(String(e)); if (!confirm) setReview(null); }
    finally { gate.current = false; setPending(false); }
  }
  async function lookup() {
    if (gate.current || busy || !submitted) return;
    gate.current = true; setPending(true); setError(""); setStatus("");
    try {
      const i = submitted.input;
      const result = await query({ type: "readManagementCountConfirmation", projectId: i.projectId, operationId: i.operationId, aspect: i.action.kind, reviewDigest: submitted.reviewDigest });
      const found = result.confirmation;
      if (!found || found.projectId !== i.projectId || found.operationId !== i.operationId || found.aspect !== i.action.kind || found.reviewDigest !== submitted.reviewDigest || (found.record && !same(found.record.review,submitted))) throw new Error("查询回复与原确认不符，请保留原数据核查。");
      setStatus(!found.record ? "尚未找到原确认；未自动重发。" : found.record.state === "recorded" ? "原确认已记入账本。请刷新计数清单。" : "原确认已保存，账本待发布，请保留数据并重开核心核对。");
    } catch(e) { setError(String(e)); } finally { gate.current = false; setPending(false); }
  }
  return <section className="panel" aria-label="核对计数操作"><h3>核对 {entry.operationId}</h3>
    <fieldset disabled={busy || pending || !!submitted}>
      <label>处理事项<select aria-label="处理事项" value={kind} onChange={e => edit(() => setKind(e.target.value as typeof kind))}>
        <option value="settleFee" disabled={!!entry.feeResolution}>核销计数费用</option><option value="retireContinuation" disabled={!!entry.retirement}>终止原操作续发</option>
      </select></label>
      {kind === "settleFee" && <><label>实际计数金额（USD）<input aria-label="实际计数金额（USD）" value={amount} onChange={e => edit(() => setAmount(e.target.value))}/></label>
        <label>计数账单引用<input aria-label="计数账单引用" value={reference} maxLength={8000} onChange={e => edit(() => setReference(e.target.value))}/></label></>}
      <label>计数核对说明<textarea aria-label="计数核对说明" value={explanation} maxLength={8000} onChange={e => edit(() => setExplanation(e.target.value))}/></label>
      <button disabled={kind === "settleFee" ? !!entry.feeResolution : !!entry.retirement} onClick={() => void act(false)}>审阅计数处理</button>
    </fieldset>
    {review && !submitted && <section aria-label="计数处理审阅"><p>项目：{review.input.projectId}；原结果：{outcome(review.originalOutcome)}；原计数预留：${decimalAmount(review.maximumUsdNanos,9)}</p>
      {review.input.action.kind === "settleFee" ? <><p>核销金额：${decimalAmount(review.input.action.amountUsdNanos,9)}；引用：{review.input.action.billingReference}</p><p>{review.input.action.explanation}</p><p>仅释放原计数预留并计入此金额。工作台未向供应商验证账单；本次不会解除续发待审阅状态。</p></> : <><p>{review.input.action.reason}</p><p>永久终止该操作续发，不释放计数或生成费用，不重置请求次数。</p></>}
      <details><summary>原记录引用</summary><p>计数来源：<code>{review.sourceDigest}</code></p><p>生成记录：<code>{review.generationDigest}</code></p><p>原账本：<code>{review.previousLedgerHead}</code></p><p>预算：<code>{review.policyDigest}</code></p></details>
      <button disabled={busy || pending} onClick={() => void act(true)}>确认本次计数处理</button>
    </section>}
    {submitted && <><p>已提交审阅：<code>{submitted.reviewDigest}</code>。不自动重发。</p><button disabled={busy || pending} onClick={() => void lookup()}>只读查询原计数确认</button></>}
    {error && <p role="alert"><ErrorText error={error}/></p>}{status && <p role="status">{status}</p>}
    <button disabled={pending} onClick={close}>关闭计数核对</button>
  </section>;
}
