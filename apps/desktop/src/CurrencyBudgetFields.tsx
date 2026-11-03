import { ErrorText } from "./ErrorText";
import { useEffect, useState } from "react";
import type { ApiTarget, BudgetSettings, PriceQuote, ProjectSetupDraft } from "./contracts";
import { ScopedCurrencyBudgetFields, ScopedCurrencyBudgetReview } from "./ScopedCurrencyBudgetFields";

const safeInteger = 9007199254740991n;
export function parseDecimal(value: string, decimals: number, maximum = safeInteger): number {
  if (!new RegExp(`^\\d+(?:\\.\\d{0,${decimals}})?$`).test(value)) throw new Error(`请输入非负金额，最多 ${decimals} 位小数。`);
  const [whole, fraction = ""] = value.split(".");
  const units = BigInt(whole) * 10n ** BigInt(decimals) + BigInt(fraction.padEnd(decimals, "0"));
  if (units > maximum) throw new Error("金额超出支持范围。");
  return Number(units);
}
export function decimalAmount(units: number, decimals: number): string {
  const text = String(units).padStart(decimals + 1, "0");
  const fraction = text.slice(-decimals).replace(/0+$/, "");
  return text.slice(0, -decimals) + (fraction ? "." + fraction : "");
}
function targetName(target: ApiTarget): string {
  if (target.kind === "openAi") return "OpenAI 官方 API";
  return target.endpoint.kind;
}
type Row = { target: ApiTarget; model: string; reported: string; input: string; output: string; read: string; write: string; from: string; until: string; source: string; countFee?: string; countBody?: string; countSource?: string };
function row(quote: PriceQuote): Row {
  return { target: quote.target, model: quote.requestedModel, reported: quote.reportedModels.join("\n"),
    input: decimalAmount(quote.rates.input, 6), output: decimalAmount(quote.rates.output, 6),
    read: quote.rates.cacheRead === null ? "" : decimalAmount(quote.rates.cacheRead, 6),
    write: quote.rates.cacheWrite === null ? "" : decimalAmount(quote.rates.cacheWrite, 6),
    from: new Date(quote.validFrom).toISOString().slice(0, 19), until: new Date(quote.validUntil).toISOString().slice(0, 19), source: quote.source,
    countFee: quote.countPrice ? decimalAmount(quote.countPrice.maximumUsdNanos, 9) : "", countBody: quote.countPrice ? String(quote.countPrice.maximumBodyBytes) : "", countSource: quote.countPrice?.source ?? "" };
}
function models(draft: ProjectSetupDraft): { target: ApiTarget; model: string }[] {
  const candidates = [{ target: { kind: "openAi" } as ApiTarget, model: draft.chiefModel },
    { target: { kind: "openAi" } as ApiTarget, model: draft.deputyModel },
    ...(draft.apiWorker ? [{ target: draft.apiWorker.target, model: draft.apiWorker.model }] : []),
    ...(draft.profiles ?? []).flatMap(p => p.apiWorker ? [{ target: p.apiWorker.target, model: p.apiWorker.model }] : [])];
  const seen = new Set<string>();
  return candidates.filter(c => { const key = JSON.stringify(c); if (!c.model || seen.has(key)) return false; seen.add(key); return true; });
}
function collect(amount: string, rows: Row[], draft: ProjectSetupDraft): BudgetSettings {
  if (!draft.apiWorker || (draft.profiles ?? []).some(p => !p.apiWorker)) throw new Error("整个项目的金额上限需要可计量的 API 执行者；Agent/桌面订阅费用请继续使用时间与次数限制。");
  const limitUsdNanos = parseDecimal(amount, 9);
  if (!limitUsdNanos) throw new Error("金额上限必须大于零。");
  if (!rows.length || rows.length > 64) throw new Error("请补充主副指挥及执行模型的报价。");
  const quotes = rows.map((r): PriceQuote => {
    const reportedModels = r.reported.split(/\r?\n/).map(v => v.trim()).filter(Boolean);
    if (!reportedModels.length || reportedModels.length > 16 || new Set(reportedModels).size !== reportedModels.length || reportedModels.some(v => !/^[A-Za-z0-9_.:/-]{1,200}$/.test(v))) throw new Error("请填写允许返回的具体模型 ID，每行一个且不能重复。");
    if (!r.source.trim() || r.source.length > 1024 || /[\x00-\x1f\x7f]/.test(r.source)) throw new Error("请填写报价来源说明。");
    if (!r.from || !r.until) throw new Error("请填写报价生效和失效时间。");
    const validFrom = new Date(r.from + "Z").toISOString(), validUntil = new Date(r.until + "Z").toISOString();
    if (Date.parse(validUntil) <= Date.parse(validFrom)) throw new Error("报价失效时间必须晚于生效时间。");
    const rate = (s: string) => parseDecimal(s, 6, 1000000000000n);
    let countPrice: PriceQuote["countPrice"];
    if (r.countFee || r.countBody || r.countSource) {
      if (r.target.kind !== "openAi") throw new Error("计数报价仅适用于 OpenAI 官方主副指挥请求。");
      const maximumUsdNanos = parseDecimal(r.countFee ?? "", 9);
      if (!/^[1-9]\d*$/.test(r.countBody ?? "") || Number(r.countBody) > 262144) throw new Error("计数报价适用请求大小须为 1 至 262144 字节的整数。");
      if (!r.countSource?.trim() || r.countSource.length > 1024 || /[\x00-\x1f\x7f]/.test(r.countSource)) throw new Error("请填写计数费用上限的独立来源说明。");
      countPrice = { schema: 1, maximumUsdNanos, maximumBodyBytes: Number(r.countBody), source: r.countSource };
    }
    return { schema: 1, target: r.target, requestedModel: r.model, reportedModels, validFrom, validUntil, source: r.source,
      ...(countPrice ? { countPrice } : {}),
      rates: { input: rate(r.input), output: rate(r.output), cacheRead: r.read === "" ? null : rate(r.read), cacheWrite: r.write === "" ? null : rate(r.write) } };
  });
  if (models(draft).some(m => !quotes.some(q => q.requestedModel === m.model && JSON.stringify(q.target) === JSON.stringify(m.target)))) throw new Error("当前配置有模型尚未报价，请按当前模型补充报价。");
  return { schema: 1, limitUsdNanos, quotes };
}

export type CurrencyBudgetProps = {
  value?: BudgetSettings | null; resetValue?: BudgetSettings | null; draft: ProjectSetupDraft; resetKey: string;
  onChange: (value: BudgetSettings | undefined) => void; onInvalid: (message: string | undefined) => void;
};
export function CurrencyBudgetFields(props: CurrencyBudgetProps) {
  if (props.value?.schema === 2) return <ScopedCurrencyBudgetFields {...props} value={props.value} />;
  return <><LegacyCurrencyBudgetFields {...props} />{!props.value && <button type="button" className="secondary-button" onClick={() => {
    props.onChange({ schema: 2, limitUsdNanos: 0, quotes: [], scopedQuotes: [] });
    props.onInvalid("请补齐累计金额和服务档位、上下文报价。");
  }}>配置服务档位与上下文报价</button>}</>;
}
function LegacyCurrencyBudgetFields({ value, resetValue, draft, resetKey, onChange, onInvalid }: CurrencyBudgetProps) {
  const [amount, setAmount] = useState(""); const [rows, setRows] = useState<Row[]>([]); const [error, setError] = useState<string>();
  useEffect(() => {
    setAmount(resetValue ? decimalAmount(resetValue.limitUsdNanos, 9) : ""); setRows(resetValue?.quotes.map(row) ?? []); setError(undefined); onInvalid(undefined);
  }, [resetKey]);
  function publish(nextAmount: string, nextRows: Row[]) {
    setAmount(nextAmount); setRows(nextRows);
    try { const next = collect(nextAmount, nextRows, draft); setError(undefined); onInvalid(undefined); onChange(next); }
    catch (e) { const message = e instanceof Error ? e.message : "报价无效。"; setError(message); onInvalid(message); }
  }
  function updateRow(index: number, patch: Partial<Row>) { publish(amount, rows.map((r, i) => i === index ? { ...r, ...patch } : r)); }
  let currentError = error;
  if (value) {
    try { collect(amount, rows, draft); currentError = undefined; }
    catch (e) { currentError = e instanceof Error ? e.message : "报价无效。"; }
  } else currentError = undefined;
  useEffect(() => { onInvalid(currentError); }, [currentError, onInvalid]);
  return <section className="panel" aria-label="项目金额预算">
    <h3>项目金额预算</h3>
    <label className="checkbox-label"><input id="currency-enabled" type="checkbox" checked={!!value} onChange={e => {
      if (!e.target.checked) { onChange(undefined); onInvalid(undefined); setError(undefined); }
      else { onChange({ schema: 1, limitUsdNanos: 0, quotes: [] }); publish(amount, rows); }
    }} />设置整个项目累计的美元限额和报价</label>
    {value && <>
      <p role="status">配置可保存供审阅；当前版本尚不能执行使用金额预算的项目。保存不会调用模型。</p>
      <label htmlFor="currency-limit">累计金额上限（美元）</label><input id="currency-limit" inputMode="decimal" required value={amount} onChange={e => publish(e.target.value, rows)} />
      <p className="secondary">主指挥、副指挥和 API 执行者共用额度。修订方案不清空已花费金额或未知请求的预留。这里按报价核算，实际账单由供应商确认。</p>
      <button type="button" className="secondary-button" onClick={() => {
        const added = models(draft).filter(m => !rows.some(r => r.model === m.model && JSON.stringify(r.target) === JSON.stringify(m.target)))
          .map(m => ({ ...m, reported: m.model, input: "", output: "", read: "", write: "", from: "", until: "", source: "" }));
        publish(amount, [...rows, ...added]);
      }}>按当前模型补充报价</button>
      {rows.map((r, i) => <fieldset key={JSON.stringify(r.target) + r.model} className="panel">
        <legend>{targetName(r.target)} · {r.model}</legend>
        {([['input', '普通输入'], ['output', '输出'], ['read', '缓存读取（未知可留空）'], ['write', '缓存写入（未知可留空）']] as const).map(([key, name]) => <div key={key}>
          <label htmlFor={`currency-${i}-${key}`}>{name} · 美元 / 百万 token</label><input id={`currency-${i}-${key}`} inputMode="decimal" required={key === 'input' || key === 'output'} value={r[key]} onChange={e => updateRow(i, { [key]: e.target.value })} />
        </div>)}
        <label htmlFor={`currency-${i}-models`}>允许返回的具体模型 ID（每行一个）</label><textarea id={`currency-${i}-models`} required value={r.reported} onChange={e => updateRow(i, { reported: e.target.value })} />
        <label htmlFor={`currency-${i}-from`}>生效时间（UTC）</label><input id={`currency-${i}-from`} type="datetime-local" step="1" required value={r.from} onChange={e => updateRow(i, { from: e.target.value })} />
        <label htmlFor={`currency-${i}-until`}>失效时间（UTC）</label><input id={`currency-${i}-until`} type="datetime-local" step="1" required value={r.until} onChange={e => updateRow(i, { until: e.target.value })} />
        <label htmlFor={`currency-${i}-source`}>报价来源说明</label><input id={`currency-${i}-source`} required value={r.source} onChange={e => updateRow(i, { source: e.target.value })} />
        {r.target.kind === "openAi" && <>
          <label htmlFor={`currency-${i}-countFee`}>单次计数费用上限（美元，未知留空）</label><input id={`currency-${i}-countFee`} inputMode="decimal" value={r.countFee ?? ""} onChange={e => updateRow(i, { countFee: e.target.value })} />
          <label htmlFor={`currency-${i}-countBody`}>计数报价适用请求大小上限（字节）</label><input id={`currency-${i}-countBody`} inputMode="numeric" value={r.countBody ?? ""} onChange={e => updateRow(i, { countBody: e.target.value })} />
          <label htmlFor={`currency-${i}-countSource`}>计数费用上限来源说明</label><input id={`currency-${i}-countSource`} value={r.countSource ?? ""} onChange={e => updateRow(i, { countSource: e.target.value })} />
          <p className="secondary">计数和生成共用累计限额。计数返回成功也不代表费用已结清，仍保留预留，等待账单核对。留空不代表免费；只有明确确认零费用时才填 0。</p>
        </>}
        <button type="button" className="secondary-button" onClick={() => publish(amount, rows.filter((_, index) => index !== i))}>移除此模型报价</button>
      </fieldset>)}
      <p className="secondary">留空表示价格未知，不能当作免费；可能发生的费用必须先有报价才能预留。切换端点或模型后，请补充对应报价。</p>
      {currentError && <p role="alert"><ErrorText error={currentError}/></p>}
    </>}
  </section>;
}

export function CurrencyBudgetReview({ value }: { value: BudgetSettings }) {
  if (value.schema === 2) return <ScopedCurrencyBudgetReview value={value} />;
  return <section aria-label="批准的金额预算"><h4>项目累计上限：${decimalAmount(value.limitUsdNanos, 9)}</h4>
    <p>包含已花费金额和未决预留；修订不会清零。按以下报价核算，未知费用不计为零。</p>
    {value.quotes.map((q, i) => <div key={i}><strong>{targetName(q.target)} · {q.requestedModel}</strong>
      <p>输入 ${decimalAmount(q.rates.input, 6)} / 输出 ${decimalAmount(q.rates.output, 6)}（每百万 token）；缓存读 {q.rates.cacheRead === null ? "未知" : "$" + decimalAmount(q.rates.cacheRead, 6)} / 写 {q.rates.cacheWrite === null ? "未知" : "$" + decimalAmount(q.rates.cacheWrite, 6)}</p>
      {q.target.kind === "openAi" && <p>计数费用上限：{q.countPrice ? `$${decimalAmount(q.countPrice.maximumUsdNanos, 9)} / 次；适用请求不超过 ${q.countPrice.maximumBodyBytes} 字节；来源：${q.countPrice.source}。成功返回后仍保留预留，待账单核对。` : "未知，不能按免费请求发送。"}</p>}
      <p>允许返回：{q.reportedModels.join("、")}；有效期 {q.validFrom} 至 {q.validUntil}；来源：{q.source}</p></div>)}
  </section>;
}
