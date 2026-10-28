import { useEffect, useId, useRef, useState } from "react";
import type { Command, CurrencyHistoryGap, HistoricalAccounting, HistoricalChargeInput, HistoricalChargeReview, HistoricalScope, ProjectCurrencyBudget, HistoricalResult } from "./contracts";
import { decimalAmount, parseDecimal } from "./CurrencyBudgetFields";
import { HistoricalRecord } from "./HistoricalRecord";

export type HistoricalQuery = (command: Command) => Promise<HistoricalResult | null>;
export function sameHistoricalScope(a: HistoricalScope, b: HistoricalScope) {
  return a.kind === "management" && b.kind === "management" ? a.operationId === b.operationId && a.originalProjectId === b.originalProjectId
    : a.kind === "api" && b.kind === "api" && a.projectId === b.projectId && a.attemptId === b.attemptId;
}
function sameInput(a: HistoricalChargeInput, b: HistoricalChargeInput) {
  return a.projectId === b.projectId && sameHistoricalScope(a.scope, b.scope) && a.amountUsdNanos === b.amountUsdNanos
    && a.billingReference === b.billingReference && a.explanation === b.explanation;
}
export function historyScope(g: CurrencyHistoryGap): HistoricalScope {
  if (g.reason === "apiWithoutBudget") {
    if (!g.projectId) throw new Error("原 API 项目缺失，请核对原记录。");
    return { kind: "api", projectId: g.projectId, attemptId: g.sourceId };
  }
  return { kind: "management", operationId: g.sourceId, originalProjectId: g.projectId };
}
export function HistoricalChargeForm({ gap, accounts, busy, query, close }: {
  gap: CurrencyHistoryGap; accounts: ProjectCurrencyBudget[]; busy: boolean; query: HistoricalQuery; close: (saved: boolean) => void;
}) {
  const [project, setProject] = useState(gap.projectId ?? "");
  const [amount, setAmount] = useState(""); const [reference, setReference] = useState(""); const [explanation, setExplanation] = useState("");
  const [review, setReview] = useState<HistoricalChargeReview | null>(null); const [error, setError] = useState(""); const [pending, setPending] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [submitted, setSubmitted] = useState<HistoricalChargeReview | null>(null);
  const [found, setFound] = useState<HistoricalAccounting | null>(null);
  const [missing, setMissing] = useState(false);
  const active = useRef(true); const sending = useRef(false);
  const formId = useId();
  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  const book = accounts.find(a => a.projectId === project);
  const valid = review && book?.healthy && book.approvalLinked && book.ledgerHead === review.previousLedgerHead;
  function edit(action: () => void) { action(); setReview(null); setError(""); }
  function input(): HistoricalChargeInput {
    if (!project || !reference.trim() || !explanation.trim()) throw new Error("请选择归属项目，并填写账单引用和核对说明。");
    return { projectId: project, scope: historyScope(gap), amountUsdNanos: parseDecimal(amount, 9), billingReference: reference, explanation };
  }
  async function submit(confirm: boolean) {
    if (sending.current || busy || submitted) return;
    try {
      const prepared = input();
      if (confirm && (!valid || !sameInput(prepared, review.input))) throw new Error("审阅内容或账本已变化，请重新审阅。");
      sending.current = true; setPending(true); setConfirming(confirm); setError("");
      if (confirm) setSubmitted(review);
      const result = await query(confirm ? { type: "confirmHistoricalCharge", input: prepared, reviewDigest: review!.reviewDigest } : { type: "reviewHistoricalCharge", input: prepared });
      if (!active.current) return;
      if (!result) { setReview(null); setError(confirm ? "确认结果尚不明确，请查询原确认；请勿重复提交。" : "审阅未完成，请查看工作台错误信息后重试。"); return; }
      if (confirm) { close(true); return; }
      const next = result.historicalChargeReview;
      if (!next || !sameInput(next.input, prepared) || !sameHistoricalScope(next.source.scope, prepared.scope)) throw new Error("返回的审阅与输入不一致。");
      setReview(next);
    } catch (e) { if (active.current) { setReview(null); setError(String(e)); } }
    finally { sending.current = false; if (active.current) { setPending(false); setConfirming(false); } }
  }
  async function readSubmitted() {
    if (!submitted || sending.current || busy) return;
    sending.current = true; setPending(true); setError(""); setMissing(false); setFound(null);
    try {
      const result = await query({ type: "readHistoricalConfirmation", projectId: submitted.input.projectId, scope: submitted.input.scope, reviewDigest: submitted.reviewDigest });
      if (!active.current) return;
      const lookup = result?.historicalConfirmation;
      if (!lookup || lookup.projectId !== submitted.input.projectId || !sameHistoricalScope(lookup.scope, submitted.input.scope) || lookup.reviewDigest !== submitted.reviewDigest)
        throw new Error("原确认未能可靠读取，请保留输入并核对核心状态；不会重新提交。");
      if (!lookup.record) { setMissing(true); return; }
      const record = lookup.record;
      if (record.review.reviewDigest !== submitted.reviewDigest || !sameInput(record.review.input, submitted.input)
        || record.review.source.digest !== submitted.source.digest || !sameHistoricalScope(record.review.source.scope, submitted.source.scope)
        || record.review.policyDigest !== submitted.policyDigest || record.review.previousLedgerHead !== submitted.previousLedgerHead
        || record.review.source.uncertainOutcomes !== submitted.source.uncertainOutcomes
        || record.review.source.references.length !== submitted.source.references.length
        || record.review.source.references.some((r, i) => r !== submitted.source.references[i]))
        throw new Error("返回的原确认与本次审阅不一致，请保留原数据核查。");
      setFound(record);
    } catch (e) { if (active.current) setError(String(e)); }
    finally { sending.current = false; if (active.current) setPending(false); }
  }
  return <section className="panel currency-history" aria-label="核对历史费用"><h3>核对 {gap.sourceId}</h3>
    <p>填写覆盖这条原调用或完整执行范围的账单金额。零金额也需要明确依据；本次记录不会释放其他请求的预留。</p>
    <fieldset disabled={pending || busy || !!submitted}>
      <label htmlFor={`${formId}-project`}>归属项目</label><select id={`${formId}-project`} value={project} disabled={gap.projectId !== null} onChange={e => edit(() => setProject(e.target.value))}>
        <option value="">请明确选择项目</option>{accounts.map(a => <option key={a.projectId} value={a.projectId}>{a.projectId}</option>)}
      </select>
      <label htmlFor={`${formId}-amount`}>账单金额（USD）</label><input id={`${formId}-amount`} inputMode="decimal" value={amount} onChange={e => edit(() => setAmount(e.target.value))}/>
      <label htmlFor={`${formId}-reference`}>账单引用</label><input id={`${formId}-reference`} value={reference} maxLength={8000} onChange={e => edit(() => setReference(e.target.value))}/>
      <label htmlFor={`${formId}-explanation`}>核对说明</label><textarea id={`${formId}-explanation`} value={explanation} maxLength={8000} onChange={e => edit(() => setExplanation(e.target.value))}/>
      <button disabled={!book?.healthy || !book.approvalLinked} onClick={() => void submit(false)}>审阅历史金额</button>
    </fieldset>
    {(!book || !book.approvalLinked) && <p role="status">请先为归属项目批准金额预算。</p>}
    {error && <p role="alert">{error}</p>}
    {review && !submitted && <section aria-label="确认历史金额"><h4>请核对后确认</h4>
      <p>项目：{review.input.projectId}；金额：${decimalAmount(review.input.amountUsdNanos, 9)}</p>
      <p>账单引用：{review.input.billingReference}</p><p>核对说明：{review.input.explanation}</p>
      <p>原结果仍有 {review.source.uncertainOutcomes} 项不确定。本次金额来自你的账单核对，工作台未向供应商验证账单。</p>
      <details><summary>查看本次完整来源</summary><ol>{review.source.references.map(r => <li key={r}><code>{r}</code></li>)}</ol>
        <p>来源摘要：<code>{review.source.digest}</code></p><p>原账户：<code>{review.previousLedgerHead}</code></p><p>原预算策略：<code>{review.policyDigest}</code></p></details>
      {!valid && <p role="status">账本或批准状态已变化，请重新审阅。</p>}
      <button className="history-confirm" disabled={pending || busy || !valid} onClick={() => void submit(true)}>确认记入历史费用</button>
    </section>}
    {pending && confirming && <p role="status">确认已提交，正在核对结果。</p>}
    {submitted && !(pending && confirming) && <section aria-label="查询已提交确认">
      <p>原审阅摘要：<code>{submitted.reviewDigest}</code></p>
      <button disabled={pending || busy} onClick={() => void readSubmitted()}>只读查询原确认</button>
      {missing && <><p role="status">尚未找到原确认；这不代表原调用免费。请核对核心状态，未自动重发。</p>
        <button disabled={pending || busy} onClick={() => { setSubmitted(null); setReview(null); setMissing(false); setError(""); }}>返回编辑并重新审阅</button></>}
      {found && <HistoricalRecord record={found}/>}
      {found?.state === "confirmedPending" && <p>请保留原数据，重开核心后再次查询金额发布结果。</p>}
      {found?.state === "accounted" && <button onClick={() => close(true)}>完成核对</button>}
    </section>}
    <button disabled={pending && confirming} onClick={() => { active.current = false; close(false); }}>{submitted ? "关闭核对" : "取消核对"}</button>
  </section>;
}
