import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { useState } from "react";
import type { ApiTarget, BlindReviewAction, Command, ReviewPhase, Snapshot } from "./contracts";
import { combineReviewRedactions, defaultReviewRedactions, readReviewRuleDraft, reviewRuleFingerprint, reviewRulesKey, reviewRulesNeedPreparation, writeReviewRuleDraft } from "./reviewRedactions";

function serviceName(target: ApiTarget | null): string {
  if (!target) return "绑定的本地会话";
  if (target.kind === "openAi") return "OpenAI 官方 API";
  return ({deepSeek:"DeepSeek 官方 API",anthropic:"Anthropic 官方 API",kimiChina:"Kimi 官方 API",grok:"Grok 官方 API",a6Api:"中转 api.a6api.com",a6ApiVip:"中转 api.a6api.vip",qwenBeijing:"Qwen 北京",qwenSingapore:"Qwen 新加坡",qwenHongKong:"Qwen 香港"} as const)[target.endpoint.kind];
}

const labels: Record<ReviewPhase, string> = {
  prepared: "等待审阅并批准", collecting: "独立评审中 · 票据密封",
  awaitingFunctionalTest: "全票通过 · 等待主指挥功能测试",
  functionalTesting: "主指挥整体功能测试中",
  functionalTestFailed: "整体功能测试未通过 · 需要核对或修订",
  awaitingUser: "功能测试通过 · 等待用户核对交付", rejected: "未通过 · 需要修订",
  incomplete: "技术未完成", cancelled: "已取消", superseded: "原交付已变化",
  accepted: "用户已确认最终验收", changesRequested: "已请求主指挥修订",
};
export function BlindReviewPanel({ data, busy, send }: {
  data: Snapshot | null; busy: boolean; send: (command: Command) => Promise<boolean>;
}) {
  const [projectId, setProject] = useState("");
  const [, refreshRules] = useState(0);
  const rulesKey = reviewRulesKey(data, projectId);
  const draft = readReviewRuleDraft(rulesKey);
  const { rules, aliases } = draft;
  const defaults = defaultReviewRedactions(data, projectId);
  const [reason, setReason] = useState("");
  const [error, setError] = useState("");
  const [reviewed, setReviewed] = useState<string | null>(null);
  function editRules(values: Partial<typeof draft>) {
    writeReviewRuleDraft(rulesKey, { ...draft, ...values, edited: true });
    refreshRules(n => n + 1);
    setReviewed(null);
    setError("");
  }
  const act = (action: BlindReviewAction) => send({ type: "blindReview", action });
  async function prepare() {
    setError("");
    let redactions;
    try { redactions = combineReviewRedactions(defaults, rules); }
    catch (error) { setError(String(error instanceof Error ? error.message : error)); return; }
    const modelAliases: Record<string,string[]> = Object.create(null);
    for (const line of aliases.split("\n").filter(s => s.trim())) {
      const parts=line.split("=>").map(s=>s.trim());
      if (parts.length!==2 || !/^(chief|deputy|worker-[1-5])$/.test(parts[0]) || !parts[1]) {setError("模型版本规则请写成：chief（或deputy、worker-1） => 供应商准确版本名称");return;}
      (modelAliases[parts[0]] ??= []).push(parts[1]);
    }
    const fingerprint = reviewRuleFingerprint(data, projectId);
    const pending = { ...draft, edited: true };
    writeReviewRuleDraft(rulesKey, pending);
    refreshRules(n => n + 1);
    setReviewed(null);
    if (await act({ kind: "prepare", projectId, redactions, modelAliases })) {
      // A delayed response must not mark newer edits (or another project) ready.
      if (readReviewRuleDraft(rulesKey) === pending) writeReviewRuleDraft(rulesKey, { ...pending, prepared: fingerprint });
      refreshRules(n => n + 1);
    }
  }
  const latest = new Map((data?.blindReviews ?? []).map(r => [r.projectId, r]));
  return <section className="panel blind-review-panel" aria-label="项目盲评">
    <HelpHeading as="h2" topic="BlindReviewPanel">项目盲评与最终验收</HelpHeading>
    <p>统一独立检查核对各执行端的稳定提交和最终集成源码，检查报告归档后由副指挥确认就绪。主、副指挥和每个执行模型各一票，全票及双权重门槛通过后开放主指挥整体功能测试，最后由你核对交付产物。</p>
    <p>核心按角色转发批准的源码片段，票据收齐前保持密封。API、CLI 和桌面会话分别核对来源、模型及隔离条件。</p>
    <label>项目编号<input list="blind-review-projects" value={projectId} disabled={busy} onChange={e => { setProject(e.target.value.trim()); setError(""); setReviewed(null); }} placeholder="选择或输入已完成任务验收的项目编号" /></label>
    <datalist id="blind-review-projects">{(data?.workflows ?? []).map(w => <option key={w.project.id} value={w.project.id} />)}</datalist>
    <section className="review-redactions" aria-label="脱敏规则">
      <h3>脱敏规则</h3>
      <p>默认启用文件路径匿名化、源码分片，以及疑似密钥、显式传话标记和危险控制字符检查。发现风险时停止准备，不会自动发送。</p>
      <label htmlFor="review-default-rules">默认脱敏规则（自动生成）</label>
      <textarea id="review-default-rules" rows={4} readOnly value={defaults.map(r => `${r.literal} => ${r.replacement}`).join("\n")} placeholder="选择项目后，根据已保存的项目、主指挥与执行目录自动生成" />
      <p className="secondary">{defaults.length ? `已生成 ${defaults.length} 条目录替换规则，覆盖正斜杠、反斜杠和转义写法；按原文精确匹配，较长目录优先。` : "尚未生成项目目录规则，内置检查始终保留。"}</p>
      <details><summary>自定义补充规则（可选）{rules.trim() ? " · 已填写" : ""}</summary>
        <label htmlFor="review-custom-rules">自定义脱敏规则</label>
        <textarea id="review-custom-rules" rows={5} disabled={busy || !projectId} value={rules} onChange={e => editRules({ rules: e.target.value })} placeholder="每行：原文 => 替代标记，例如：内部代号 => PRIVATE_NAME；不要在此配置模型 Key" />
        <p className="secondary">原文 3–256 字节；替代标记 3–64 位英文字母、数字、下划线或连字符。含默认最多 64 条。同一原文的自定义标记优先，内置检查不可关闭。</p>
        <button className="secondary-button" disabled={busy || !rules} onClick={() => editRules({ rules: "" })}>清空自定义规则</button>
      </details>
      <p className="secondary">自定义内容在本次打开期间按项目保留，关闭程序后清空。修改后需重新准备并审阅材料；脱敏不保证识别所有隐写暗号，请核对替换后仍能判断功能。</p>
    </section>
    <label htmlFor="review-model-aliases">允许的模型版本别名（可选）</label>
    <textarea id="review-model-aliases" disabled={busy || !projectId} value={aliases} onChange={e=>editRules({ aliases: e.target.value })} placeholder="仅填写供应商文档确认的版本名，例如：chief => 准确版本名；留空时要求返回模型名与配置完全一致" />
    <button disabled={busy || !projectId.trim()} onClick={() => void prepare()}>准备材料与完整名册</button>
    <p>准备仅在本机进行。首次使用会保护新验收数据，旧版核心将拒绝打开此数据目录，并禁止旧宿主权限执行及外部主指挥聊天。建议使用独立实验工作台，先保留已有备份。</p>
    {error && <p role="alert"><ErrorText error={error}/></p>}
    {[...latest.values()].map(r => <article key={r.roundId}>
      <HelpHeading as="h3" topic="BlindReviewPanel">{r.projectId} · {r.phase === "accepted" && !r.finalAccepted ? "历史轮次已确认，当前交付未核实" : labels[r.phase]}</HelpHeading>
      <p>{r.fileCount} 个文件，{r.fragmentCount} 个片段，材料 {r.materialBytes} 字节。每席一票，需要 {r.seats.length}/{r.seats.length} 通过。</p>
      {r.checkArchiveDigest && <details><summary>统一独立检查 · 报告已归档</summary><p>已核对各任务的批准检查、断言与源码封存，完整项目使用最终集成交付的检查记录。源码改变后原报告失效，不拼接多个执行目录冒充整体验收。</p><p>归档摘要 <code>{r.checkArchiveDigest}</code></p></details>}
      {r.weightPolicy && <section aria-label="加权验收判据">
        <h4>加权验收 · 判据 v{r.weightPolicy.version}</h4>
        <p>指令遵循 {r.weightPolicy.instructionShare / 100}% ＋ 独立检查 {r.weightPolicy.checkShare / 100}%；单票至少 {r.weightPolicy.minimumSeat / 100}%，全体平均至少 {r.weightPolicy.minimumOverall / 100}%，并保留全票通过要求。</p>
        <p className="secondary">校准状态：尚未校准。指令分来自模型对批准标准的逐项核对及有效分片引用；检查分来自真实检查记录。这是可复核的证据分，不代表准确率或测试覆盖率。缺失项不计分；工具违规或过程证据不足的票计零分，仍保留在总票数中。</p>
        {!r.weightedResult && <p>票据收齐前不展示逐票分数或总分。</p>}
        {r.weightedResult && <>
          <p>全体平均 {(r.weightedResult.overallBasisPoints / 100).toFixed(2)}% · {r.weightedResult.passed ? "双门槛通过" : "未通过"}</p>
          <table><thead><tr><th>席位</th><th>指令分</th><th>检查分</th><th>综合分</th><th>结论与缺失项</th></tr></thead><tbody>{r.weightedResult.seats.map(s => <tr key={s.seat}><td>{s.seat}</td><td>{s.instructionBasisPoints / 100}%</td><td>{s.checkBasisPoints / 100}%</td><td>{s.totalBasisPoints / 100}%</td><td>{s.passed ? "通过" : "未通过"}{s.veto && ` · ${s.veto}`}{s.missingCriteria.length > 0 && ` · 缺少标准 ${s.missingCriteria.join("、")}`}{s.unalignedCriteria.length > 0 && ` · 未对齐标准 ${s.unalignedCriteria.join("、")}`}</td></tr>)}</tbody></table>
        </>}
      </section>}
      <ul>{r.seats.map(s => <li key={s.id}>{s.role === "chief" ? "主指挥" : s.role === "deputy" ? "副指挥" : "执行端"}：{s.model} · {serviceName(s.target)}；允许的返回名称：{s.allowedModels.join("、")}{s.target && <span> · 每次输出上限 {s.maxOutputTokens ?? 4096} tokens</span>}</li>)}</ul>
      {r.message && <p>{r.message}</p>}
      {!r.isCurrent && <p>当前交付或证据无法与本轮绑定一致；不能以此轮结果作出当前项目通过结论。</p>}
      <details><summary>完整交付范围与排除项（仅向用户展示原路径）</summary>
        <p>集成任务：{r.coverage.integrationTask ?? "同一交付目录"}</p>
        <pre>{r.coverage.files.join("\n")}</pre>
        <p>批准范围中的排除项：{r.coverage.excluded.join("、") || "无"}。请确认排除项不包含所需源码、配置或依赖；如有遗漏，应回主指挥修订方案。</p>
      </details>
      {r.phase === "prepared" && <>
        {reviewRulesNeedPreparation(data, r.projectId) && <p role="status">此项目的规则已修改，请先取消本轮，再重新准备材料并审阅批准。</p>}
        <p>核心最多发送 {r.maxRequests} 条逻辑消息，含分片接收确认、副指挥就绪确认和未通过后的一次分析。API 每次输出最多 {r.maxOutputTokens} tokens、{r.timeoutSeconds} 秒，会话回复最多读取16KiB；整轮15分钟。可能按量计费，没有金额封顶，本地应用的供应商请求由其自身配置管理。</p>
        <details><summary>查看将发送的完整脱敏材料</summary><pre style={{ maxHeight: 360, overflow: "auto", whiteSpace: "pre-wrap" }}>{JSON.stringify(r.materialPreview, null, 2)}</pre></details>
        <label><input type="checkbox" disabled={busy || reviewRulesNeedPreparation(data, r.projectId)} checked={reviewed === r.digest && !reviewRulesNeedPreparation(data, r.projectId)} onChange={e => setReviewed(e.target.checked ? r.digest : null)} />我已核对材料覆盖、脱敏结果、名册和调用范围</label>
        <button disabled={busy || !r.isCurrent || reviewed !== r.digest || reviewRulesNeedPreparation(data, r.projectId)} onClick={() => void act({ kind: "start", projectId: r.projectId, roundId: r.roundId, digest: r.digest })}>批准并开始本轮评审</button>
      </>}
      {r.ballots && <ul>{r.ballots.map(b => <li key={b.seat}>{b.seat} · {b.model} · {b.ballot.vote === "pass" ? "通过" : "未通过"}。{b.ballot.reason}{b.ballot.citations.length > 0 && `（${b.ballot.citations.join("、")}）`}
        {b.toolAudit && <details><summary>评分过程工具核验</summary><p>{b.toolAudit.status === "clean" ? "完整协议记录未观察到工具调用" : b.toolAudit.status === "toolCallObserved" ? "违规调用工具 · 本票判为未通过" : "核验依据不足 · 本票判为未通过"}</p><p>{b.toolAudit.reason}</p><p>{b.toolAudit.tools.join("、")}</p>{b.originalBallot && <p>模型原始评分为{b.originalBallot.vote === "pass" ? "通过" : "未通过"}。{b.originalBallot.reason}</p>}<p>证据摘要 <code>{b.toolAudit.evidenceDigest}</code></p></details>}
      </li>)}</ul>}
      {r.analysisStatus && <p>{r.analysisStatus}</p>}
      {r.deputyAnalysis && <details open><summary>副指挥的修订请求</summary>
        <p>遗漏：{r.deputyAnalysis.omissions.join("；") || "未指出"}</p>
        <p>验收标准疑议：{r.deputyAnalysis.disputedCriteria.join("；") || "无"}</p>
        <p>{r.deputyAnalysis.chiefRevisionRequest}</p>
        <p>这是修订建议。进入主指挥问答后，仍需审阅并批准新方案，才会继续执行。</p>
      </details>}
      <section className="panel" aria-label="主指挥整体功能测试">
        <h4>整体功能测试 · 固定关联主指挥</h4>
        <p>{r.seats.find(s => s.role === "chief")?.model} · {r.phase === "awaitingFunctionalTest" ? "盲评全票通过，权限已开放" : r.phase === "functionalTesting" ? "主指挥单独审阅检查范围、运行封存副本检查并核对结果" : r.functionalTest?.status === "passed" ? "整体功能测试已通过" : "按本轮阶段核对测试许可"}</p>
        {r.functionalTest?.message && <p>{r.functionalTest.message}</p>}
        {r.functionalTest?.decision && <p>{r.functionalTest.decision.reason}</p>}
        {!!r.functionalTest?.checks.length && <ul>{r.functionalTest.checks.map(c => <li key={c.checkId}>{c.checkId} · {c.exitCode === 0 && !c.timedOut && !c.assertionError ? "通过" : "未通过"} · {c.assertions?.length ?? 0} 项断言</li>)}</ul>}
        {r.phase === "awaitingFunctionalTest" && <>
          <p>固定使用本轮主指挥，分别确认检查范围和核对真实结果。API 每段请求一次，会话接入按分片发送并核对回执。核心按已批准参数执行一次整体交付检查，结果保留在本机。可能按量计费。</p>
          {r.assembledDelivery && <button disabled={busy || !r.isCurrent} onClick={() => void act({kind:"prepareFunctionalEnvironment",projectId:r.projectId,roundId:r.roundId,digest:r.digest})}>准备独立检查账户</button>}
          <button disabled={busy || !r.isCurrent} onClick={() => void act({kind:"startFunctionalTest",projectId:r.projectId,roundId:r.roundId,digest:r.digest})}>由主指挥开始整体功能测试</button>
        </>}
      </section>
      {r.phase === "awaitingUser" && r.functionalTest?.status === "passed" && r.confirmationDigest && <button disabled={busy || !r.isCurrent} onClick={() => void act({ kind: "confirm", projectId: r.projectId, roundId: r.roundId, digest: r.confirmationDigest! })}>交付无需修改，确认最终验收</button>}
      {["prepared", "collecting", "awaitingFunctionalTest", "functionalTesting", "awaitingUser"].includes(r.phase) && <button disabled={busy} onClick={() => void act({ kind: "cancel", projectId: r.projectId, roundId: r.roundId })}>取消本轮</button>}
      {["rejected", "awaitingFunctionalTest", "functionalTestFailed", "awaitingUser", "accepted"].includes(r.phase) && <>
        <label>需要修改的内容<textarea value={reason} onChange={e => setReason(e.target.value)} /></label>
        <button disabled={busy || !reason.trim()} onClick={() => void act({ kind: "requestChanges", projectId: r.projectId, roundId: r.roundId, reason })}>交主指挥问答并修订方案</button>
      </>}
    </article>)}
  </section>;
}
