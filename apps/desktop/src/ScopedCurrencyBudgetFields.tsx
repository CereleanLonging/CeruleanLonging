import { useEffect, useState } from "react";
import type { BudgetSettings, PriceBand, ProjectSetupDraft, RequestedService, ScopedPriceQuote } from "./contracts";
import { decimalAmount, parseDecimal, type CurrencyBudgetProps } from "./CurrencyBudgetFields";

type BandRow = { model: string; service: string; from: string; through: string; input: string; output: string; read: string; write: string };
type QuoteRow = { model: string; requested: RequestedService; models: string; services: string; bands: BandRow[]; from: string; until: string; source: string; countFee: string; countBody: string; countSource: string };
const idPattern = /^[A-Za-z0-9_.:/-]{1,200}$/;
const servicePattern = /^[A-Za-z0-9_.-]{1,64}$/;
const sourceValid = (s: string) => !!s.trim() && s.length <= 1024 && !/[\x00-\x1f\x7f]/.test(s);
const rate = (s: string) => parseDecimal(s, 6, 1000000000000n);
const maybeRate = (s: string) => s === "" ? null : rate(s);
function integer(s: string, maximum: number) { if (!/^\d+$/.test(s) || !Number.isSafeInteger(Number(s)) || Number(s) > maximum) throw new Error("请输入范围内的非负整数。"); return Number(s); }
function list(s: string, pattern: RegExp) {
  const values = s.split(/\r?\n/).map(v => v.trim()).filter(Boolean);
  if (!values.length || values.length > 16 || values.some(v => !pattern.test(v)) || new Set(values).size !== values.length) throw new Error("模型或档位列表需为 1 至 16 个不重复的有效值。");
  return values;
}
function models(draft: ProjectSetupDraft) {
  const workers = [draft.apiWorker, ...(draft.profiles ?? []).map(p => p.apiWorker)];
  if (workers.some(w => !w || w.target.kind !== "openAi")) throw new Error("当前分档金额预算支持所有执行者使用 OpenAI 官方 API 的项目。");
  return [...new Set([draft.chiefModel, draft.deputyModel, ...workers.map(w => w!.model)].filter(Boolean))];
}
function row(q: ScopedPriceQuote): QuoteRow {
  return { model: q.requestedModel, requested: q.requestedService, models: q.reportedModels.join("\n"), services: q.reportedServices.join("\n"),
    bands: q.bands.map(b => ({ model: b.actualModel, service: b.actualService, from: String(b.input.from), through: String(b.input.through), input: decimalAmount(b.rates.input, 6), output: decimalAmount(b.rates.output, 6), read: b.rates.cacheRead === null ? "" : decimalAmount(b.rates.cacheRead, 6), write: b.rates.cacheWrite === null ? "" : decimalAmount(b.rates.cacheWrite, 6) })),
    from: q.validFrom, until: q.validUntil, source: q.source,
    countFee: q.countPrice ? decimalAmount(q.countPrice.maximumUsdNanos, 9) : "", countBody: q.countPrice ? String(q.countPrice.maximumBodyBytes) : "", countSource: q.countPrice?.source ?? "" };
}
function newBand(model: string, service: string): BandRow { return { model, service, from: "", through: "", input: "", output: "", read: "", write: "" }; }
function newRow(model: string): QuoteRow { return { model, requested: { kind: "explicit", tier: "default" }, models: model, services: "default", bands: [newBand(model, "default")], from: "", until: "", source: "", countFee: "", countBody: "", countSource: "" }; }
function collect(amount: string, rows: QuoteRow[], draft: ProjectSetupDraft): BudgetSettings {
  const requiredModels = models(draft);
  const limitUsdNanos = parseDecimal(amount, 9);
  if (!limitUsdNanos || !rows.length || rows.length > 64) throw new Error("请填写正数累计上限和各模型的分档报价。");
  const scopedQuotes = rows.map((r): ScopedPriceQuote => {
    if (!idPattern.test(r.model)) throw new Error("请求模型 ID 无效。");
    const reportedModels = list(r.models, idPattern), reportedServices = list(r.services, servicePattern);
    if (reportedServices.includes("auto")) throw new Error("auto 是请求选择方式，不能作为已知的实际计费档位。");
    if (!sourceValid(r.source)) throw new Error("请填写有效的报价来源。");
    if (!/^\d{4}-\d{2}-\d{2}T.+(?:Z|[+-]\d{2}:\d{2})$/.test(r.from) || !/^\d{4}-\d{2}-\d{2}T.+(?:Z|[+-]\d{2}:\d{2})$/.test(r.until) || !Number.isFinite(Date.parse(r.from)) || !Number.isFinite(Date.parse(r.until)) || Date.parse(r.from) >= Date.parse(r.until)) throw new Error("有效期需为带时区的 RFC3339 时间，结束时间必须晚于开始时间。");
    if (!r.bands.length || r.bands.length > 512) throw new Error("每份报价需要 1 至 512 个价格区间。");
    const bands: PriceBand[] = r.bands.map(b => {
      const from = integer(b.from, 100000000), through = integer(b.through, 100000000);
      if (from > through || !reportedModels.includes(b.model) || !reportedServices.includes(b.service)) throw new Error("区间起点不能大于终点，且需使用列表中的实际模型与档位。");
      return { actualModel: b.model, actualService: b.service, input: { from, through }, rates: { input: rate(b.input), output: rate(b.output), cacheRead: maybeRate(b.read), cacheWrite: maybeRate(b.write) } };
    });
    for (const model of reportedModels) for (const service of reportedServices) {
      const subset = bands.filter(b => b.actualModel === model && b.actualService === service).sort((a, b) => a.input.from - b.input.from);
      if (!subset.length || subset.some((b, i) => i > 0 && subset[i - 1].input.through >= b.input.from)) throw new Error("每种实际模型与档位都需要报价，同一组合的区间不能重叠。");
    }
    let countPrice: ScopedPriceQuote["countPrice"];
    if (r.countFee !== "" || r.countBody !== "" || r.countSource !== "") {
      if (r.countFee === "") throw new Error("计数费用条款不完整；请填写费用上限，或清空三项计数条款。");
      const maximumBodyBytes = integer(r.countBody, 262144);
      if (!maximumBodyBytes || !sourceValid(r.countSource)) throw new Error("计数费用需要请求大小上限和独立来源。");
      countPrice = { schema: 1, maximumUsdNanos: parseDecimal(r.countFee, 9), maximumBodyBytes, source: r.countSource };
    }
    return { schema: 1, target: { kind: "openAi" }, requestedModel: r.model, requestedService: r.requested, reportedModels, reportedServices, billingBasis: "perRequest", contextMetric: "totalInputIncludingCache", bands, validFrom: r.from, validUntil: r.until, source: r.source, ...(countPrice ? { countPrice } : {}) };
  });
  if (requiredModels.some(model => !scopedQuotes.some(q => q.requestedModel === model))) throw new Error("当前配置中的模型仍有缺失报价。");
  return { schema: 2, quotes: [], scopedQuotes, limitUsdNanos };
}

export function ScopedCurrencyBudgetFields({ value, resetValue, draft, resetKey, onChange, onInvalid }: CurrencyBudgetProps & { value: BudgetSettings }) {
  const original = resetValue?.schema === 2 ? resetValue : undefined;
  const [amount, setAmount] = useState(() => original ? decimalAmount(original.limitUsdNanos, 9) : "");
  const [rows, setRows] = useState<QuoteRow[]>(() => original?.scopedQuotes?.map(row) ?? []);
  const [notice, setNotice] = useState<string>();
  useEffect(() => { setAmount(original ? decimalAmount(original.limitUsdNanos, 9) : ""); setRows(original?.scopedQuotes?.map(row) ?? []); setNotice(undefined); }, [resetKey]);
  let validation: string | undefined;
  try { collect(amount, rows, draft); } catch (e) { validation = e instanceof Error ? e.message : "分档报价无效。"; }
  useEffect(() => onInvalid(validation), [validation, onInvalid]);
  function publish(nextAmount: string, nextRows: QuoteRow[]) {
    setAmount(nextAmount); setRows(nextRows); setNotice(undefined);
    try { onChange(collect(nextAmount, nextRows, draft)); onInvalid(undefined); }
    catch (e) { onInvalid(e instanceof Error ? e.message : "分档报价无效。"); }
  }
  const update = (i: number, patch: Partial<QuoteRow>) => publish(amount, rows.map((r, j) => j === i ? { ...r, ...patch } : r));
  return <section className="panel" aria-label="项目分档金额预算">
    <h3>服务档位与上下文报价</h3>
    <label className="checkbox-label"><input id="scoped-currency-enabled" type="checkbox" checked={!!value} onChange={() => { onChange(undefined); onInvalid(undefined); }} />设置整个项目累计限额</label>
    <p role="status">配置可保存供审阅；当前版本尚不能执行使用金额预算的项目。保存不会调用模型。</p>
    <label htmlFor="scoped-currency-limit">累计上限（美元）</label><input id="scoped-currency-limit" inputMode="decimal" value={amount} onChange={e => publish(e.target.value, rows)} />
    <p>输入区间包含普通输入、缓存读取和缓存写入，起点和终点均包含。价格按整次请求计算；会话累计重定价需另外核对。省略的价格或用量不能当作零。</p>
    <button type="button" onClick={() => { try { publish(amount, [...rows, ...models(draft).filter(model => !rows.some(r => r.model === model)).map(newRow)]); } catch (e) { setNotice(e instanceof Error ? e.message : "配置不支持"); } }}>补充当前模型的分档报价</button>
    {rows.map((r, i) => <fieldset className="panel" key={i}><legend>OpenAI · {r.model}</legend>
      <p>请求档位：{r.requested.kind === "explicit" ? r.requested.tier : "未指定，随项目设置"}。实际档位以响应为准。</p>
      {([['models', '允许返回的具体模型（每行一个）'], ['services', '允许返回的实际服务档位（每行一个）']] as const).map(([key, label]) => <div key={key}><label htmlFor={`scoped-${i}-${key}`}>{label}</label><textarea id={`scoped-${i}-${key}`} value={r[key]} onChange={e => update(i, { [key]: e.target.value })} /></div>)}
      {r.bands.map((b, k) => <fieldset key={k}><legend>价格区间 {k + 1}</legend>{([['model', '实际模型'], ['service', '实际档位'], ['from', '输入起点（token）'], ['through', '输入终点（token）'], ['input', '普通输入 · 美元/百万 token'], ['output', '输出 · 美元/百万 token'], ['read', '缓存读取（未知留空）'], ['write', '缓存写入（未知留空）']] as const).map(([key, label]) => <div key={key}><label htmlFor={`scoped-${i}-band-${k}-${key}`}>{label}</label><input id={`scoped-${i}-band-${k}-${key}`} value={b[key]} onChange={e => update(i, { bands: r.bands.map((v, index) => index === k ? { ...v, [key]: e.target.value } : v) })} /></div>)}
        <button type="button" className="secondary-button" onClick={() => update(i, { bands: r.bands.filter((_, j) => j !== k) })}>移除区间 {k + 1}</button></fieldset>)}
      <button type="button" onClick={() => update(i, { bands: [...r.bands, newBand(r.models.split(/\r?\n/)[0].trim(), r.services.split(/\r?\n/)[0].trim())] })}>添加价格区间</button>
      {([['from', '生效时间（例如 2026-10-16T00:00:00+08:00）'], ['until', '失效时间（带时区）'], ['source', '报价来源'], ['countFee', '单次计数费用上限（美元，未知留空）'], ['countBody', '计数请求大小上限（字节）'], ['countSource', '计数费用独立来源']] as const).map(([key, label]) => <div key={key}><label htmlFor={`scoped-${i}-${key}`}>{label}</label><input id={`scoped-${i}-${key}`} value={r[key]} onChange={e => update(i, { [key]: e.target.value })} /></div>)}
      <p className="secondary">计数成功仍保留计数费预留，待账单核对。输入 0 仅表示明确审阅了零费用条款。</p>
      <button type="button" className="secondary-button" onClick={() => publish(amount, rows.filter((_, j) => j !== i))}>移除此模型报价</button>
    </fieldset>)}
    {(notice ?? validation) && <p role="alert">{notice ?? validation}</p>}
  </section>;
}

export function ScopedCurrencyBudgetReview({ value }: { value: BudgetSettings }) {
  return <section className="scoped-price-review" aria-label="批准的金额预算"><h4>项目累计上限：${decimalAmount(value.limitUsdNanos, 9)}</h4><p>主副指挥和执行者共用额度。按原批准报价核算；未知费用保留预留。以下条款按每次请求计算。</p>
    {(value.scopedQuotes ?? []).map((q, i) => <div key={i}><strong>OpenAI · {q.requestedModel}</strong><p>请求档位：{q.requestedService.kind === "explicit" ? q.requestedService.tier : "未指定"}；允许实际档位：{q.reportedServices.join("、")}；允许实际模型：{q.reportedModels.join("、")}</p>
      <div className="table-scroll"><table><thead><tr><th>实际模型/档位</th><th>完整输入 token</th><th>输入/输出（美元/百万）</th><th>缓存读/写（美元/百万）</th></tr></thead><tbody>{q.bands.map((b, k) => <tr key={k}><td>{b.actualModel} / {b.actualService}</td><td>{b.input.from} 至 {b.input.through}（含两端）</td><td>{decimalAmount(b.rates.input, 6)} / {decimalAmount(b.rates.output, 6)}</td><td>{b.rates.cacheRead === null ? "未知" : decimalAmount(b.rates.cacheRead, 6)} / {b.rates.cacheWrite === null ? "未知" : decimalAmount(b.rates.cacheWrite, 6)}</td></tr>)}</tbody></table></div>
      <p>有效期：{q.validFrom} 至 {q.validUntil}；来源：{q.source}</p><p>计数费用：{q.countPrice ? `$${decimalAmount(q.countPrice.maximumUsdNanos, 9)} / 次；请求不超过 ${q.countPrice.maximumBodyBytes} 字节；来源：${q.countPrice.source}；成功后仍待账单核销。` : "未知，不代表免费。"}</p>
    </div>)}
  </section>;
}
