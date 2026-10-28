import { useEffect, useRef, useState } from "react";
import type { Command, CurrencyHistoryGap, CurrencyHistoryReason, HistoricalAccounting, HistoricalAccountingPage, HistoricalAccountingSummary, HistoricalSourcePage, ProjectCurrencyBudget, HistoricalResult } from "./contracts";
import { HistoricalChargeForm, sameHistoricalScope, type HistoricalQuery } from "./HistoricalChargeForm";
import { HistoricalRecord } from "./HistoricalRecord";
import { decimalAmount } from "./CurrencyBudgetFields";

type PageInfo = { revision: string; offset: number; nextOffset: number | null; total: number };
function usePage<T extends PageInfo>(incoming: T | null | undefined, select: (s: HistoricalResult) => T | null, command: (offset: number, revision: string | null) => Command, query: HistoricalQuery | undefined, busy: boolean) {
  const [saved, setSaved] = useState<T | null>(null);
  const [back, setBack] = useState<number[]>([]);
  const [reading, setReading] = useState(false); const [error, setError] = useState("");
  const inFlight = useRef(false); const active = useRef(true); const latest = useRef(incoming);
  latest.current = incoming;
  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  const fresh = !!saved && saved.revision === incoming?.revision;
  const page = fresh ? saved : incoming;
  const stack = fresh ? back : [];
  async function read(mode: "next" | "previous" | "refresh") {
    if (inFlight.current || busy || !query) return;
    const before = latest.current?.revision;
    const offset = mode === "refresh" ? 0 : mode === "next" ? page?.nextOffset : stack.at(-1);
    if (offset == null) return;
    const revision = mode === "refresh" ? null : page?.revision ?? null;
    inFlight.current = true; setReading(true); setError("");
    try {
      const result = await query(command(offset, revision));
      if (!active.current) return;
      const next = result && select(result);
      if (!next || next.offset !== offset || (revision !== null && next.revision !== revision)
        || (latest.current?.revision !== before && !(mode === "refresh" && latest.current?.revision === next.revision))) throw new Error("记录已变化或读取失败，请刷新首页；未采用迟到结果。");
      setSaved(next); setBack(mode === "refresh" ? [] : mode === "previous" ? stack.slice(0, -1) : [...stack, page!.offset]);
    } catch (e) { if (active.current) setError(String(e)); }
    finally { inFlight.current = false; if (active.current) setReading(false); }
  }
  return { page, changed: !!saved && !fresh, error, reading, controls: <div>
    <button disabled={busy || reading || !query || !stack.length} onClick={() => void read("previous")}>上一页</button>
    <span> {page ? `${page.total ? page.offset + 1 : 0} 起 / 共 ${page.total} 条` : "清单暂不可用"} </span>
    <button disabled={busy || reading || !query || page?.nextOffset == null} onClick={() => void read("next")}>下一页</button>
    <button disabled={busy || reading || !query} onClick={() => void read("refresh")}>刷新首页</button>
  </div> };
}
const reasons: Record<CurrencyHistoryReason, string> = {
  legacyManagementRequest: "旧管理请求缺少完整计费依据", managementWithoutBudget: "管理请求未采用金额预算",
  reviewedManagementRecord: "原管理记录曾有残缺", apiWithoutBudget: "历史 API 执行未采用金额预算",
};

export function HistoricalPages({ sources, accounting, accounts, query, busy }: {
  sources?: HistoricalSourcePage | null; accounting?: HistoricalAccountingPage | null;
  accounts: ProjectCurrencyBudget[]; query?: HistoricalQuery; busy: boolean;
}) {
  const source = usePage(sources, s => s.historicalSources, (offset, revision) => ({ type: "readHistoricalSources", offset, revision }), query, busy);
  const amounts = usePage(accounting, s => s.historicalAccounting, (offset, revision) => ({ type: "readHistoricalAccounting", offset, revision }), query, busy);
  const [selected, setSelected] = useState<CurrencyHistoryGap | null>(null);
  const [notice, setNotice] = useState(""); const [error, setError] = useState("");
  const [detail, setDetail] = useState<{ record: HistoricalAccounting; revision: string } | null>(null);
  const [reading, setReading] = useState(false);
  const inFlight = useRef(false); const active = useRef(true); const latestRevision = useRef(accounting?.revision);
  latestRevision.current = accounting?.revision;
  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  async function readDetail(summary: HistoricalAccountingSummary) {
    if (busy || inFlight.current || !query || !amounts.page) return;
    const revision = amounts.page.revision;
    inFlight.current = true; setReading(true); setError(""); setDetail(null);
    try {
      const result = await query({ type: "readHistoricalConfirmation", projectId: summary.projectId, scope: summary.scope, reviewDigest: summary.reviewDigest });
      if (!active.current) return;
      const lookup = result?.historicalConfirmation; const record = lookup?.record;
      if (!lookup || !record || lookup.projectId !== summary.projectId || !sameHistoricalScope(lookup.scope, summary.scope)
        || lookup.reviewDigest !== summary.reviewDigest || record.review.reviewDigest !== summary.reviewDigest
        || record.userEventDigest !== summary.userEventDigest || record.state !== summary.state
        || record.review.input.projectId !== summary.projectId || !sameHistoricalScope(record.review.input.scope, summary.scope)
        || record.review.input.amountUsdNanos !== summary.amountUsdNanos || record.review.source.digest !== summary.sourceDigest
        || result?.historicalAccounting?.revision !== revision || latestRevision.current !== revision)
        throw new Error("原确认或列表版本已变化，未展示过期详情；请刷新清单后重新查询。");
      setDetail({ record, revision });
    } catch (e) { if (active.current) setError(String(e)); }
    finally { inFlight.current = false; if (active.current) setReading(false); }
  }
  return <>
    <section className="panel currency-history" aria-label="历史来源清单"><h2>历史费用来源</h2>
      {notice && <p role="status">{notice}</p>}
      <p>原记录保留 {source.page?.total ?? "待核对"} 条，待核算 {source.page?.pendingTotal ?? "待核对"} 条。这些条目不是收费请求数量，也不代表免费。</p>
      {!source.page && <p role="status">历史来源清单暂不可用，请保留原数据并核对核心状态。</p>}
      {source.changed && <p role="status">历史来源已变化，已显示当前首页。</p>}
      {source.error && <p role="alert">{source.error}</p>}
      <ol start={(source.page?.offset ?? 0) + 1}>{source.page?.entries.map(({ gap, accounted }) => <li key={`${gap.projectId}:${gap.sourceId}:${gap.reference}`}>
        <p>{gap.projectId === null ? "未归属项目" : `项目 ${gap.projectId}`} · {gap.sourceId}：{reasons[gap.reason]}</p>
        <p>{accounted === true ? "已核算，原来源保留" : accounted === false ? "费用待核对" : "核算状态暂不可用"}</p>
        <p>原记录引用：<code>{gap.reference}</code></p>
        {accounted === false && <button disabled={busy || reading || !query} onClick={() => { setSelected(gap); setNotice(""); }}>核对 {gap.sourceId}</button>}
      </li>)}</ol>{source.controls}
    </section>
    {selected && query && <HistoricalChargeForm key={`${selected.projectId}:${selected.sourceId}:${selected.reference}`} gap={selected} accounts={accounts} busy={busy} query={query} close={saved => { setSelected(null); if (saved) setNotice("历史金额已记录，原始来源继续保留。"); }}/>} 
    <section className="panel currency-history" aria-label="历史核算清单"><h2>已确认历史金额</h2>
      {!amounts.page && <p role="status">历史核算清单暂不可用，请保留原数据并核对核心状态。</p>}
      {amounts.changed && <p role="status">历史核算记录已变化，已显示当前首页。</p>}
      {amounts.error && <p role="alert">{amounts.error}</p>}{error && <p role="alert">{error}</p>}
      {amounts.page?.entries.map(r => <article key={r.userEventDigest}>
        <h3>项目 {r.projectId} · ${decimalAmount(r.amountUsdNanos, 9)}</h3>
        <p>{r.state === "accounted" ? "已核算" : "用户已确认，金额发布待核对"}；原结果不确定项：{r.uncertainOutcomes}</p>
        <p>原范围：{r.scope.kind === "management" ? r.scope.operationId : r.scope.attemptId}</p>
        <button disabled={busy || reading || !query} onClick={() => void readDetail(r)}>查看原确认 {r.scope.kind === "management" ? r.scope.operationId : r.scope.attemptId}</button>
      </article>)}{amounts.controls}
      {detail && detail.revision === accounting?.revision && <HistoricalRecord record={detail.record}/>}
      {detail && detail.revision !== accounting?.revision && <p role="status">详情对应的清单已变化，请重新查询原确认。</p>}
    </section>
  </>;
}
