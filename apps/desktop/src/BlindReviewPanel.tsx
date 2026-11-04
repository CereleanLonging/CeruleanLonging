import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { useState } from "react";
import type { ApiTarget, BlindReviewAction, Command, ReviewPhase, Snapshot } from "./contracts";

function serviceName(target: ApiTarget): string {
  if (target.kind === "openAi") return "OpenAI 官方 API";
  return ({deepSeek:"DeepSeek 官方 API",anthropic:"Anthropic 官方 API",kimiChina:"Kimi 官方 API",grok:"Grok 官方 API",a6Api:"中转 api.a6api.com",a6ApiVip:"中转 api.a6api.vip",qwenBeijing:"Qwen 北京",qwenSingapore:"Qwen 新加坡",qwenHongKong:"Qwen 香港"} as const)[target.endpoint.kind];
}

const labels: Record<ReviewPhase, string> = {
  prepared: "等待审阅并批准", collecting: "独立评审中 · 票据密封",
  awaitingUser: "全票通过 · 等待用户确认", rejected: "未通过 · 需要修订",
  incomplete: "技术未完成", cancelled: "已取消", superseded: "原交付已变化",
  accepted: "用户已确认最终验收", changesRequested: "已请求主指挥修订",
};
export function BlindReviewPanel({ data, busy, send }: {
  data: Snapshot | null; busy: boolean; send: (command: Command) => Promise<boolean>;
}) {
  const [projectId, setProject] = useState("");
  const [rules, setRules] = useState("");
  const [aliases, setAliases] = useState("");
  const [reason, setReason] = useState("");
  const [error, setError] = useState("");
  const [reviewed, setReviewed] = useState<string | null>(null);
  const act = (action: BlindReviewAction) => send({ type: "blindReview", action });
  async function prepare() {
    setError("");
    const redactions = rules.split("\n").filter(s => s.trim()).map(line => {
      const pos = line.indexOf("=>");
      return { literal: pos < 0 ? "" : line.slice(0, pos).trim(), replacement: pos < 0 ? "" : line.slice(pos + 2).trim() };
    });
    if (redactions.some(r => !r.literal || !r.replacement)) { setError("每条脱敏规则请写成：原文 => 替代标记"); return; }
    const modelAliases: Record<string,string[]> = Object.create(null);
    for (const line of aliases.split("\n").filter(s => s.trim())) {
      const parts=line.split("=>").map(s=>s.trim());
      if (parts.length!==2 || !/^(chief|deputy|worker-[1-5])$/.test(parts[0]) || !parts[1]) {setError("模型版本规则请写成：chief（或deputy、worker-1） => 供应商准确版本名称");return;}
      (modelAliases[parts[0]] ??= []).push(parts[1]);
    }
    if (await act({ kind: "prepare", projectId, redactions, modelAliases })) { setRules(""); setReviewed(null); }
  }
  const latest = new Map((data?.blindReviews ?? []).map(r => [r.projectId, r]));
  return <section className="panel blind-review-panel" aria-label="项目盲评">
    <HelpHeading as="h2" topic="BlindReviewPanel">项目盲评与最终验收</HelpHeading>
    <p>任务执行完成后，核对完整交付和客观检查，再由副指挥确认就绪。主、副指挥和每个执行模型各一票，全票通过后由你最终确认。</p>
    <p>评审只能接收批准的源码片段，没有文件工具、总分或其他票据。CLI、桌面或外部聊天未具备受限评审协议时，会明确停止整轮。</p>
    <label>项目编号<input list="blind-review-projects" value={projectId} onChange={e => setProject(e.target.value)} placeholder="选择或输入已完成任务验收的项目编号" /></label>
    <datalist id="blind-review-projects">{(data?.workflows ?? []).map(w => <option key={w.project.id} value={w.project.id} />)}</datalist>
    <label>可选脱敏规则<textarea value={rules} onChange={e => setRules(e.target.value)} placeholder="每行：原文 => 替代标记；不要在此配置模型Key" /></label>
    <label>允许的模型版本别名（可选）<textarea value={aliases} onChange={e=>setAliases(e.target.value)} placeholder="仅填写供应商文档确认的版本名，例如：chief => 准确版本名；留空时要求返回模型名与配置完全一致" /></label>
    <button disabled={busy || !projectId.trim()} onClick={() => void prepare()}>准备材料与完整名册</button>
    <p>准备仅在本机进行。首次使用会保护新验收数据，旧版核心将拒绝打开此数据目录，并禁止旧宿主权限执行及外部主指挥聊天。建议使用独立实验工作台，先保留已有备份。</p>
    {error && <p role="alert"><ErrorText error={error}/></p>}
    {[...latest.values()].map(r => <article key={r.roundId}>
      <HelpHeading as="h3" topic="BlindReviewPanel">{r.projectId} · {r.phase === "accepted" && !r.finalAccepted ? "历史轮次已确认，当前交付未核实" : labels[r.phase]}</HelpHeading>
      <p>{r.fileCount} 个文件，{r.fragmentCount} 个片段，材料 {r.materialBytes} 字节。每席一票，需要 {r.seats.length}/{r.seats.length} 通过。</p>
      <ul>{r.seats.map(s => <li key={s.id}>{s.role === "chief" ? "主指挥" : s.role === "deputy" ? "副指挥" : "执行端"}：{s.model} · {serviceName(s.target)}；允许的返回名称：{s.allowedModels.join("、")}</li>)}</ul>
      {r.message && <p>{r.message}</p>}
      {!r.isCurrent && <p>当前交付或证据无法与本轮绑定一致；不能以此轮结果作出当前项目通过结论。</p>}
      <details><summary>完整交付范围与排除项（仅向用户展示原路径）</summary>
        <p>集成任务：{r.coverage.integrationTask ?? "同一交付目录"}</p>
        <pre>{r.coverage.files.join("\n")}</pre>
        <p>批准范围中的排除项：{r.coverage.excluded.join("、") || "无"}。请确认排除项不包含所需源码、配置或依赖；如有遗漏，应回主指挥修订方案。</p>
      </details>
      {r.phase === "prepared" && <>
        <p>最多 {r.maxRequests} 次请求（含一次副指挥就绪确认，以及未通过后的一次副指挥分析），每次输出最多 {r.maxOutputTokens} tokens、{r.timeoutSeconds} 秒；整轮15分钟，失败不重试。可能按量计费，没有金额封顶。</p>
        <details><summary>查看将发送的完整脱敏材料</summary><pre style={{ maxHeight: 360, overflow: "auto", whiteSpace: "pre-wrap" }}>{JSON.stringify(r.materialPreview, null, 2)}</pre></details>
        <label><input type="checkbox" checked={reviewed === r.digest} onChange={e => setReviewed(e.target.checked ? r.digest : null)} />我已核对材料覆盖、脱敏结果、名册和调用范围</label>
        <button disabled={busy || !r.isCurrent || reviewed !== r.digest} onClick={() => void act({ kind: "start", projectId: r.projectId, roundId: r.roundId, digest: r.digest })}>批准并开始本轮评审</button>
      </>}
      {r.ballots && <ul>{r.ballots.map(b => <li key={b.seat}>{b.seat} · {b.model}：{b.ballot.vote === "pass" ? "通过" : "未通过"}。{b.ballot.reason}（{b.ballot.citations.join("、")}）</li>)}</ul>}
      {r.analysisStatus && <p>{r.analysisStatus}</p>}
      {r.deputyAnalysis && <details open><summary>副指挥的修订请求</summary>
        <p>遗漏：{r.deputyAnalysis.omissions.join("；") || "未指出"}</p>
        <p>验收标准疑议：{r.deputyAnalysis.disputedCriteria.join("；") || "无"}</p>
        <p>{r.deputyAnalysis.chiefRevisionRequest}</p>
        <p>这是修订建议。进入主指挥问答后，仍需审阅并批准新方案，才会继续执行。</p>
      </details>}
      {r.phase === "awaitingUser" && <button disabled={busy || !r.isCurrent} onClick={() => void act({ kind: "confirm", projectId: r.projectId, roundId: r.roundId, digest: r.digest })}>交付无需修改，确认最终验收</button>}
      {["prepared", "collecting", "awaitingUser"].includes(r.phase) && <button disabled={busy} onClick={() => void act({ kind: "cancel", projectId: r.projectId, roundId: r.roundId })}>取消本轮</button>}
      {["rejected", "awaitingUser", "accepted"].includes(r.phase) && <>
        <label>需要修改的内容<textarea value={reason} onChange={e => setReason(e.target.value)} /></label>
        <button disabled={busy || !reason.trim()} onClick={() => void act({ kind: "requestChanges", projectId: r.projectId, roundId: r.roundId, reason })}>交主指挥问答并修订方案</button>
      </>}
    </article>)}
  </section>;
}
