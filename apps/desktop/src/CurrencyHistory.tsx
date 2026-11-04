import { HelpHeading } from "./HelpHeading";
import { useState } from "react";
import { decimalAmount } from "./CurrencyBudgetFields";
import type { CurrencyHistoryGap, CurrencyHistoryReason, HistoricalAccounting, ProjectCurrencyBudget } from "./contracts";

import { HistoricalChargeForm, historyScope, sameHistoricalScope, type HistoricalQuery } from "./HistoricalChargeForm";

export function isHistoryAccounted(g: CurrencyHistoryGap, records?: HistoricalAccounting[] | null) {
  if (g.reason === "apiWithoutBudget" && !g.projectId) return false;
  const scope = historyScope(g);
  return records?.some(r => r.state === "accounted" && sameHistoricalScope(r.review.input.scope, scope)) ?? false;
}

const reason: Record<CurrencyHistoryReason, string> = {
  legacyManagementRequest: "旧管理请求缺少完整计费依据",
  managementWithoutBudget: "管理请求未采用金额预算",
  reviewedManagementRecord: "管理记录曾有残缺，费用仍待核对",
  apiWithoutBudget: "历史 API 执行未采用金额预算",
};

export function CurrencyHistory({ history, accounted, accounts = [], busy = false, query }: { history?: CurrencyHistoryGap[] | null; accounted?: HistoricalAccounting[] | null; accounts?: ProjectCurrencyBudget[]; busy?: boolean; query?: HistoricalQuery }) {
  const [selected, setSelected] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const original = history;
  history = history?.filter(g => !isHistoryAccounted(g, accounted));
  const [requestedPage, setPage] = useState(0);
  const [requestedAccountedPage, setAccountedPage] = useState(0);
  if (!history) return <section className="panel currency-history" aria-label="待核算历史"><HelpHeading as="h2" topic="CurrencyHistory">历史费用</HelpHeading>
    <p role="status">历史费用清单暂不可用，可用额度待核对。</p></section>;
  const chosen = history.find(g => `${g.projectId ?? "?"}:${g.sourceId}:${g.reference}` === selected);
  if (!history.length && !accounted?.length) return null;
  const pages = Math.ceil(history.length / 20);
  const page = Math.max(0, Math.min(requestedPage, pages - 1));
  const accountedPages = Math.ceil((accounted?.length ?? 0) / 20);
  const accountedPage = Math.max(0, Math.min(requestedAccountedPage, accountedPages - 1));
  const unattributed = history.filter(g => g.projectId === null).length;
  return <><section className="panel currency-history" aria-label="待核算历史"><HelpHeading as="h2" topic="CurrencyHistory">历史费用待核对</HelpHeading>
    {notice && <p role="status">{notice}</p>}
    {accounted === null && <p role="status">历史核算明细暂不可用，请保留原数据并重开核对。</p>}
    <p>共 {history.length} 条记录或执行范围待核算。这不是收费请求数量，也不代表免费；关闭任务或确认残缺记录不会消除费用。</p>
    {unattributed > 0 && <p role="status">其中 {unattributed} 条尚不能归属项目，未分摊费用。项目可用额度需待这些记录核对后确认。</p>}
    <details><summary>查看待核算来源</summary>
      <ol start={page * 20 + 1}>{history.slice(page * 20, page * 20 + 20).map(g =>
        <li key={`${g.projectId ?? "?"}:${g.sourceId}:${g.reference}`}>
          <p>{g.projectId === null ? "未归属项目" : `项目 ${g.projectId}`} · {g.sourceId}：{reason[g.reason]}</p>
          <p className="secondary">原记录引用：<code>{g.reference}</code></p>
          {query && <button disabled={busy} onClick={() => { setSelected(`${g.projectId ?? "?"}:${g.sourceId}:${g.reference}`); setNotice(""); }}>核对 {g.sourceId}</button>}
        </li>)}</ol>
      {pages > 1 && <div><button disabled={page === 0} onClick={() => setPage(page - 1)}>上一页</button>
        <span> 第 {page + 1} / {pages} 页 </span>
        <button disabled={page + 1 === pages} onClick={() => setPage(page + 1)}>下一页</button></div>}
    </details>
  </section>
    {chosen && query && <HistoricalChargeForm key={selected} gap={chosen} accounts={accounts} busy={busy} query={query} close={saved => {setSelected(null);if(saved)setNotice("历史金额已记录，原始来源继续保留。");}}/>}
    {!!accounted?.length && <section className="panel currency-history" aria-label="已确认历史金额"><HelpHeading as="h2" topic="CurrencyHistory">已确认历史金额</HelpHeading><p>原始历史来源保留 {original?.length ?? 0} 条；以下金额保留原结果的不确定性。</p>
      {accounted.slice(accountedPage * 20, accountedPage * 20 + 20).map(r => <article key={r.userEventDigest}><HelpHeading as="h3" topic="CurrencyHistory">项目 {r.review.input.projectId} · ${decimalAmount(r.review.input.amountUsdNanos,9)}</HelpHeading>
        <p>{r.state === "accounted" ? "已核算" : "用户已确认，金额发布待核对"}；原结果不确定项：{r.review.source.uncertainOutcomes}</p>
        <p>账单引用：{r.review.input.billingReference}</p><p>核对说明：{r.review.input.explanation}</p>
        <details><summary>查看已确认原记录</summary><p>用户确认：<code>{r.userEventDigest}</code></p><p>来源摘要：<code>{r.review.source.digest}</code></p><ol>{r.review.source.references.map(s => <li key={s}><code>{s}</code></li>)}</ol></details>
      </article>)}
      {accountedPages > 1 && <div><button disabled={accountedPage === 0} onClick={() => setAccountedPage(accountedPage - 1)}>上一页</button>
        <span> 第 {accountedPage + 1} / {accountedPages} 页 </span><button disabled={accountedPage + 1 === accountedPages} onClick={() => setAccountedPage(accountedPage + 1)}>下一页</button></div>}
    </section>}
  </>;
}
