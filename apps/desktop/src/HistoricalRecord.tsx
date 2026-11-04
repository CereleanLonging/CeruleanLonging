import { HelpHeading } from "./HelpHeading";
import type { HistoricalAccounting } from "./contracts";
import { decimalAmount } from "./CurrencyBudgetFields";

export function HistoricalRecord({ record }: { record: HistoricalAccounting }) {
  const { review, state, userEventDigest } = record;
  return <section aria-label="原确认详情">
    <HelpHeading as="h3" topic="HistoricalRecord">项目 {review.input.projectId} · ${decimalAmount(review.input.amountUsdNanos, 9)}</HelpHeading>
    <p role="status">{state === "accounted" ? "已核算" : "用户确认已保存，金额发布待核对"}；原结果不确定项：{review.source.uncertainOutcomes}</p>
    <p>账单引用：{review.input.billingReference}</p><p>核对说明：{review.input.explanation}</p>
    <p>用户确认：<code>{userEventDigest}</code></p><p>审阅摘要：<code>{review.reviewDigest}</code></p>
    <details><summary>查看原确认完整来源</summary><p>来源摘要：<code>{review.source.digest}</code></p>
      <ol>{review.source.references.map((r, i) => <li key={`${i}:${r}`}><code>{r}</code></li>)}</ol>
    </details>
  </section>;
}
