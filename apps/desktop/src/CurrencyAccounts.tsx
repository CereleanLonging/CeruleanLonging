import type { CurrencyHistoryGap, ProjectCurrencyBudget, HistoricalAccounting, HistoricalSourcePage, HistoricalAccountingPage } from "./contracts";
import { decimalAmount } from "./CurrencyBudgetFields";
import type { HistoricalQuery } from "./HistoricalChargeForm";
import { CurrencyHistory, isHistoryAccounted } from "./CurrencyHistory";
import { HistoricalPages } from "./HistoricalPages";

export function CurrencyAccounts({ accounts, history, accounted, sources, accounting, query, busy = false }: { accounts: ProjectCurrencyBudget[], history?: CurrencyHistoryGap[] | null; accounted?: HistoricalAccounting[] | null; sources?: HistoricalSourcePage | null; accounting?: HistoricalAccountingPage | null; query?: HistoricalQuery; busy?: boolean }) {
  const usd = (value: number) => "$" + decimalAmount(value, 9);
  const paged = sources !== undefined || accounting !== undefined;
  return <>{paged ? <HistoricalPages sources={sources} accounting={accounting} accounts={accounts} query={query} busy={busy}/> : <CurrencyHistory history={history} accounted={accounted} accounts={accounts} query={query} busy={busy}/>}{accounts.length > 0 && <section className="workflow-plans" aria-label="项目累计金额账本"><h2>项目累计金额</h2>
    <p className="secondary">按批准报价核算；实际供应商账单仍需核对。当前版本尚未开放带金额预算的模型请求。</p>
    {accounts.map(a => {
      const pending = paged ? undefined : history?.filter(g => (g.projectId === null || g.projectId === a.projectId) && !isHistoryAccounted(g, accounted)).length;
      const available = paged ? (sources && sources.pendingTotal !== null && accounting ? a.availableUsdNanos : null) : pending === 0 ? a.availableUsdNanos : null;
      return <article className="panel" key={a.projectId} aria-label={`项目 ${a.projectId} 金额`}>
      <h3>项目 {a.projectId}</h3>
      <dl><dt>累计上限</dt><dd>{usd(a.limitUsdNanos)}</dd><dt>已核算费用</dt><dd>{usd(a.chargedUsdNanos)}</dd>
        <dt>未释放预留</dt><dd>{usd(a.reservedUsdNanos)}</dd><dt>可用额度</dt><dd>{available === null ? "待核对，暂不可用" : usd(available)}</dd></dl>
      {!paged && pending === undefined && <p role="status">尚未取得完整历史费用清单。</p>}
      {paged && available === null && <p role="status">完整历史及账本仍待核对，暂不确认可用额度。</p>}
      {!!pending && <p role="status">另有 {pending} 条相关或未归属的历史记录待核算，暂不确认可用额度。</p>}
      <p>结果未确定的请求：{a.unknownRequests}；无法定价的请求：{a.unpricedRequests}。这些请求不会因为修改方案而清零。</p>
      {a.managementCounts && <div aria-label="指挥计数记录">
        <p>计数等待结果：{a.managementCounts.pending}；已取得收据：{a.managementCounts.counted}；结果未知：{a.managementCounts.unknown}；已计数但未完成生成交接：{a.managementCounts.awaitingGeneration}。</p>
        <p>计数费用预留：{usd(a.managementCounts.reservedUsdNanos)}，已包含在未释放预留中。取得计数收据或生成完成后仍保留，待供应商账单核对。</p>
        {a.managementCounts.needsReview && <p role="alert">计数交接需要审阅，已暂停该项目。请保留原计数记录；重新打开不会自动重计、发送或退款。</p>}
      </div>}
      {a.apiCounts && <div aria-label="API 执行计数记录">
        <p>API 计数等待结果：{a.apiCounts.pending}；已取得收据：{a.apiCounts.counted}；结果未知：{a.apiCounts.unknown}；已计数但尚无生成预留：{a.apiCounts.countedWithoutGeneration}。</p>
        <p>API 计数费用预留：{usd(a.apiCounts.reservedUsdNanos)}，已包含在未释放预留中，与生成费用分别保留。取得计数收据或生成完成不代表计数免费。</p>
        <p className="secondary">请在“API 计数费用核对”中审阅原费用；指挥计数核对不处理这些记录。读取状态不会重新计数、生成或释放预留。</p>
      </div>}
      {!a.approvalLinked && <p role="status">当前规划许可或方案批准与账本尚未关联，不允许执行。</p>}
      {!a.healthy && <p role="alert">账本写入尚未完整确认，请保留原数据并重开核对。</p>}
      {a.blocked && <p role="alert">存在无法定价或超出预留范围的费用，已阻止新增支出。已核算为零不代表请求免费。</p>}
      <details><summary>查看原账本引用</summary><p>{a.planningSetupDigest ? "原规划配置" : "原批准方案"}：<code>{a.planningSetupDigest??a.revisionDigest}</code></p><p>账本摘要：<code>{a.ledgerHead}</code></p></details>
    </article>;})}
  </section>}</>;
}
