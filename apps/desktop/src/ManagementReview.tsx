import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { supervisionDeliverySummary } from "./SupervisionFields";
import { useEffect, useRef, useState } from "react";
import type { Command, ManagementSummary, ManagementView, Snapshot } from "./contracts";
import "./ManagementReview.css";
import { AdviceTracking } from "./AdviceTracking";

const states: Record<string, string> = {
  awaitingExternal: "等待绑定的 Codex 聊天回填",
  running: "指挥请求进行中", unknown: "请求结果待核对", ready: "回复待处理",
  applying: "回复正在入账", applied: "回复已处理", discarded: "已放弃本地请求", abandoned: "应用事务已关闭", startupClosed: "启动记录已关闭",
};
const purposes: Record<string, string> = { plan: "主指挥规划", assign: "副指挥分配", advise: "副指挥建议", accept: "主指挥收口" };
const maxInputBytes = 8000;
const pageSize = 50;
type HistoryPage = Pick<Snapshot, "managementDecisions" | "managementHistory">;
type Action = "discard" | "reply" | "close";
type Review = { decision: ManagementView; action: Action; projectRoot: string; revision: string };
type ApplicationRecovery = NonNullable<ManagementView["applicationRecovery"]>;
function canAct(decision: ManagementView, action: Action) {
  return action === "close" ? !!decision.applicationRecovery : action === "discard" ? decision.canDiscard : decision.canReply;
}
function ApplicationTarget({ candidate }: { candidate: ApplicationRecovery }) {
  const targets: Record<string, string> = { propose: "提交方案", assign: "分配任务", accept: "收口任务", advise: "提交建议", wait: "等待补充" };
  return <dl className="management-identifiers management-application-target" aria-label="应用事务目标">
    <dt>应用目标</dt><dd>{targets[candidate.targetKind] ?? candidate.targetKind}（{candidate.targetKind}）</dd>
    <dt>任务</dt><dd>{candidate.taskId ?? "未关联任务"}</dd>
    <dt>尝试</dt><dd><code>{candidate.attemptId ?? "未关联尝试"}</code></dd>
    <dt>会话</dt><dd><code>{candidate.sessionId ?? "未关联会话"}</code></dd>
    <dt>工作流记录序号</dt><dd>{candidate.workflowSequence}</dd>
  </dl>;
}
function historyPage(snapshot: Snapshot): HistoryPage {
  return { managementDecisions: snapshot.managementDecisions, managementHistory: snapshot.managementHistory };
}
function FullText({ decision }: { decision: ManagementView }) {
  return <>
    {decision.supervisionAdvice&&<section className="management-saved-text" aria-label="副指挥监督建议"><HelpHeading as="h4" topic="ManagementReview">副指挥监督建议</HelpHeading>
      <p>任务 {decision.supervisionAdvice.taskId} · 尝试 {decision.supervisionAdvice.attemptId}</p>
      {decision.supervisionAdvice.omp && <p>OMP 原请求 {decision.supervisionAdvice.omp.requestId}</p>}
      {decision.supervisionAdvice.api && <p>API 请求 {decision.supervisionAdvice.api.requestId} · 第 {decision.supervisionAdvice.api.round} 轮</p>}
      {decision.supervisionAdvice.deliveryState === "recordOnly" && !decision.supervisionAdvice.omp && <p>仅保存供审阅，尚未自动投送；不代表执行器已经纠偏。</p>}
      <p>{decision.supervisionAdvice.current?"原观察仍对应当前运行":"原观察已过期或执行已结束，仅供审阅"} · {supervisionDeliverySummary(decision.supervisionAdvice)}</p>
      {decision.supervisionAdvice.suggestion?<><p className="management-text">{decision.supervisionAdvice.suggestion}</p><HelpHeading as="h4" topic="ManagementReview">建议依据</HelpHeading><p className="management-text">{decision.supervisionAdvice.rationale}</p></>:<p>尚无可确认的建议正文；请求结果未知时不会自动重发。</p>}
      <details><summary>关联异常</summary><p><code>{decision.supervisionAdvice.incidentId}</code></p><p><code>{decision.supervisionAdvice.observationDigest}</code></p>{decision.supervisionAdvice.omp && <p>原文件工具记录 <code>{decision.supervisionAdvice.omp.toolHead}</code></p>}</details>
    </section>}
    {decision.waitReason !== null && <div className="management-saved-text"><HelpHeading as="h4" topic="ManagementReview">指挥等待原因（原文）</HelpHeading><p className="management-text">{decision.waitReason}</p></div>}
    {decision.userReply !== null && <div className="management-saved-text"><HelpHeading as="h4" topic="ManagementReview">已保存的用户答复</HelpHeading><p className="management-text">{decision.userReply}</p><p className="secondary">答复供后续指挥请求参考；不改变已批准范围，也不自动恢复调度。</p></div>}
    {decision.discardReason !== null && <div className="management-saved-text"><HelpHeading as="h4" topic="ManagementReview">{["abandoned", "startupClosed"].includes(decision.state) ? "关闭原因" : "放弃本地请求的理由"}</HelpHeading><p className="management-text">{decision.discardReason}</p><p className="secondary">{decision.state === "startupClosed" ? "启动记录已在本地关闭，不发送任务、不自动恢复调度，也不退回请求额度；此前可能已经建立远端会话，此处不代表远程取消或退费。" : decision.state === "abandoned" ? "应用事务已在本地关闭，不发送任务、不自动恢复调度，也不退回已记录的请求额度。" : "本地请求已关闭。这不代表供应商已取消处理或退费，系统不会重发此请求。"}</p></div>}
    {!decision.supervisionAdvice && decision.waitReason === null && decision.userReply === null && decision.discardReason === null && <p className="secondary">这条请求没有已保存的等待原因、用户答复或放弃理由。</p>}
  </>;
}

export function ManagementReview({ data, send, query, busy, focusOperation }: {
  focusOperation?: {operationId:string;token:number}|null;
  data: Snapshot | null;
  send: (command: Command) => Promise<boolean>;
  query: (command: Command) => Promise<Snapshot | null>;
  busy: boolean;
}) {
  const openedFocus=useRef<number|null>(null);
  const [savedPage, setSavedPage] = useState<HistoryPage | null>(null);
  const [detail, setDetail] = useState<{ value: ManagementView; revision: string } | null>(null);
  const [review, setReview] = useState<Review | null>(null);
  const [input, setInput] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const inFlight = useRef(false);
  const readInFlight = useRef(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const detailPanel = useRef<HTMLElement>(null);
  const history = savedPage?.managementHistory;
  const decisions = savedPage?.managementDecisions ?? [];
  const pageFresh = !!history && history.revision === data?.managementHistory.revision;
  const detailFresh = !!detail && detail.revision === data?.managementHistory.revision;
  const latestSummary = review && data?.managementDecisions.find(d => d.operationId === review.decision.operationId);
  const reviewedApplication = review?.action === "close" ? review.decision.applicationRecovery : null;
  const current = !!review && review.revision === data?.managementHistory.revision
    && detail?.value.operationId === review.decision.operationId
    && detail.value.evidenceDigest === review.decision.evidenceDigest
    && canAct(detail.value, review.action)
    && (review.action !== "close" || (!!reviewedApplication
      && detail.value.applicationRecovery?.evidenceDigest === reviewedApplication.evidenceDigest
      && data?.workflows.find(w => w.project.id === review.decision.projectId)?.sequence === reviewedApplication.workflowSequence))
    && (!latestSummary || (latestSummary.evidenceDigest === review.decision.evidenceDigest
      && (review.action === "close" ? latestSummary.state === "applying" : review.action === "discard" ? latestSummary.canDiscard : latestSummary.canReply)));
  const text = input.trim();
  const inputBytes = new TextEncoder().encode(text).length;
  const hasNul = text.includes("\0");
  const pending = busy || submitting || reading;
  const projectRoot = (id: string) => data?.workflows.find(w => w.project.id === id)?.project.root ?? id;

  useEffect(() => {
    if (!savedPage && data) setSavedPage(historyPage(data));
  }, [savedPage, data]);

  useEffect(() => {
    if (detail && !review && !reading && !submitting) detailPanel.current?.scrollIntoView({ block: "start" });
  }, [detail, review, reading, submitting]);

  useEffect(() => {
    if (!review || !dialog.current) return;
    const opener = document.activeElement;
    const modal = dialog.current;
    if (!modal.open) modal.showModal();
    return () => {
      modal.close();
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, [review]);

  useEffect(()=>{
    if(!focusOperation||pending||openedFocus.current===focusOperation.token)return;
    openedFocus.current=focusOperation.token;
    void readDetail({operationId:focusOperation.operationId,evidenceDigest:""},undefined,undefined,true);
  },[focusOperation,pending]);

  async function readPage(projectId: string | null, offset: number, revision: string | null) {
    if (pending || readInFlight.current) return;
    readInFlight.current = true;
    setReading(true); setError(null); setNotice(null);
    try {
      const next = await query({ type: "readManagementPage", projectId, offset, revision });
      if (!next) { setError("记录未能读取。若历史已经变化，请刷新当前筛选的首页。"); return; }
      setSavedPage(historyPage(next));
      setDetail(null);
    } catch {
      setError("记录未能读取，请刷新后重试。");
    } finally {
      readInFlight.current = false; setReading(false);
    }
  }

  async function readDetail(summary: Pick<ManagementSummary, "operationId" | "evidenceDigest">, action?: Action, expectedApplicationDigest?: string, fromExecutor = false) {
    if (pending || readInFlight.current || (action && !pageFresh)) return;
    readInFlight.current = true;
    setReading(true); setError(null); setNotice(null);
    try {
      const next = await query({ type: "readManagementDetail", operationId: summary.operationId });
      const full = next?.managementDetail;
      if (!next || !full || full.operationId !== summary.operationId) {
        setError("完整记录未能读取，尚未打开操作确认。请重试。"); return;
      }
      if(fromExecutor) setSavedPage(historyPage(next));
      setDetail({ value: full, revision: next.managementHistory.revision });
      if (!action) return;
      if (full.evidenceDigest !== summary.evidenceDigest || next.managementHistory.revision !== history?.revision
        || !canAct(full, action)
        || (action === "close" && (!expectedApplicationDigest || full.applicationRecovery?.evidenceDigest !== expectedApplicationDigest))) {
        setError("这条请求或项目状态已变化，请刷新列表后重新审阅。"); return;
      }
      setInput("");
      setReview({ decision: full, action, projectRoot: projectRoot(full.projectId), revision: next.managementHistory.revision });
    } catch {
      setError("完整记录未能读取，尚未打开操作确认。请重试。");
    } finally {
      readInFlight.current = false; setReading(false);
    }
  }

  async function confirm() {
    if (!review || !current || pending || inFlight.current || !text || hasNul || inputBytes > maxInputBytes) return;
    inFlight.current = true; setSubmitting(true); setError(null); setNotice(null);
    const operationId = review.decision.operationId;
    try {
      const shared = { operationId, evidenceDigest: review.decision.evidenceDigest };
      const command: Command = review.action === "close"
        ? { type: "abandonManagementApplication", operationId, evidenceDigest: review.decision.applicationRecovery!.evidenceDigest, reason: text }
        : review.action === "discard"
          ? { type: "discardManagementDecision", ...shared, reason: text }
          : { type: "answerManagementWait", ...shared, answer: text };
      const ok = await send(command);
      if (!ok) { setError("本次操作未完成，请查看当前请求状态与系统提示；填写内容已保留。"); return; }
      setReview(null);
      const refreshed = await query({ type: "readManagementPage", projectId: history?.projectId ?? null, offset: 0, revision: null });
      if (refreshed) setSavedPage(historyPage(refreshed));
      const fetched = await query({ type: "readManagementDetail", operationId });
      if (fetched?.managementDetail?.operationId === operationId) {
        setDetail({ value: fetched.managementDetail, revision: fetched.managementHistory.revision });
      } else setDetail(null);
      if (!refreshed || !fetched?.managementDetail) {
        setError("操作已保存，但最新列表或详情未能完整读取，请手动刷新。");
      } else setNotice("操作已保存，当前筛选已刷新至首页，完整记录已更新。调度状态保持不变。");
    } catch {
      setError("未能确认最新操作状态。请核对记录后再处理，系统不会自动重发。");
    } finally {
      inFlight.current = false; setSubmitting(false);
    }
  }

  return <section className="workflow-plans management-review" aria-label="主副指挥请求记录">
    <HelpHeading as="h2" topic="ManagementReview">主副指挥请求记录</HelpHeading>
    <p className="secondary">每页显示 50 条请求摘要，完整记录按需读取。答复后，请在“项目持续执行”中决定是否恢复调度。</p>
    <div className="panel management-history-toolbar">
      <div><label htmlFor="management-history-project">筛选请求项目</label><select id="management-history-project" value={history?.projectId ?? ""} disabled={pending || !history} onChange={e => void readPage(e.target.value || null, 0, null)}>
        <option value="">全部项目</option>{data?.workflows.map(w => <option key={w.project.id} value={w.project.id}>{w.project.root}</option>)}
      </select></div>
      <button type="button" className="secondary-button" disabled={pending || !data} onClick={() => void readPage(history?.projectId ?? null, 0, null)}>刷新记录（回首页）</button>
    </div>
    {history && !pageFresh && <p role="status" className="management-stale">历史记录或项目状态已更新。当前页保留原来的内容，请刷新后继续翻页或处理请求。</p>}
    {error && <p role="alert" className="management-feedback"><ErrorText error={error}/></p>}
    {notice && <p role="status" className="management-feedback">{notice}</p>}
    {reading && <p role="status" className="secondary">正在读取记录…</p>}
    {detail && <article ref={detailPanel} className="panel management-detail" aria-label="请求完整记录" data-management-detail={detail.value.operationId}>
      <div className="section-heading"><HelpHeading as="h3" topic="ManagementReview">请求完整记录</HelpHeading><button type="button" className="secondary-button" disabled={pending} onClick={() => setDetail(null)}>收起完整记录</button></div>
      <dl className="management-identifiers"><dt>项目</dt><dd>{projectRoot(detail.value.projectId)}</dd><dt>请求</dt><dd><code>{detail.value.operationId}</code></dd><dt>状态</dt><dd>{states[detail.value.state] ?? detail.value.state}</dd></dl>
      {!detailFresh && <p className="management-stale">此详情读取后记录已变化，请从刷新后的列表重新查看。</p>}
      <FullText decision={detail.value}/>
      <AdviceTracking key={detail.value.operationId} decision={detail.value} data={data} send={send} query={query} busy={pending || !detailFresh || !pageFresh} refresh={async () => {
        const refreshed = await query({ type: "readManagementPage", projectId: history?.projectId ?? null, offset: 0, revision: null });
        if (refreshed) setSavedPage(historyPage(refreshed));
        const next = await query({ type: "readManagementDetail", operationId: detail.value.operationId });
        if (next?.managementDetail) { setDetail({value:next.managementDetail, revision:next.managementHistory.revision}); }
        else { setDetail(null); setError("处置已保存，请刷新列表查看最新记录。"); }
      }}/>
      {detail.value.applicationRecovery && <div className="management-saved-text">
        <HelpHeading as="h4" topic="ManagementReview">未提交的应用事务</HelpHeading><ApplicationTarget candidate={detail.value.applicationRecovery}/>
        <p className="secondary">已保存应用意图，但尚无对应工作流事件。关闭只记录本地处置，不发送任务、不自动恢复调度、不退回请求额度。</p>
        <button type="button" className="secondary-button" disabled={pending || !detailFresh || !pageFresh} onClick={() => void readDetail(detail.value, "close", detail.value.applicationRecovery!.evidenceDigest)}>关闭未提交应用…</button>
      </div>}
    </article>}
    {!decisions.length && <p className="secondary">{history?.projectId ? "该项目暂无请求记录。" : "尚无主副指挥请求记录。"}</p>}
    {decisions.map(d => <article className="panel management-entry" key={d.operationId} data-management-operation={d.operationId}>
      <div className="section-heading"><HelpHeading as="h3" topic="ManagementReview">{purposes[d.purpose] ?? d.purpose}</HelpHeading><span className="badge">{states[d.state] ?? d.state}</span></div>
      <dl className="management-identifiers"><dt>项目</dt><dd>{projectRoot(d.projectId)}</dd><dt>请求</dt><dd><code>{d.operationId}</code></dd></dl>
      {d.message && <p className="management-text">{d.message}</p>}
      <p className="secondary management-record-flags">{[d.hasWait && "有等待原因", d.hasReply && "有用户答复", d.hasDiscard && (d.state === "abandoned" ? "有关闭原因" : "有放弃理由")].filter(Boolean).join(" · ") || "请求摘要"}。全文请查看完整记录。</p>
      {d.state === "unknown" && <p className="secondary">目前没有可确认的结果，系统不会自动重发。</p>}
      <div className="card-footer management-actions">
        <button type="button" className="secondary-button" disabled={pending} onClick={() => void readDetail(d)}>查看完整记录</button>
        {d.canReply && <button type="button" className="primary" disabled={pending || !pageFresh} onClick={() => void readDetail(d, "reply")}>答复指挥等待</button>}
        {d.canDiscard && <button type="button" className="secondary-button" disabled={pending || !pageFresh} onClick={() => void readDetail(d, "discard")}>放弃本地请求…</button>}
      </div>
    </article>)}
    {history && <nav className="management-pagination" aria-label="指挥历史分页">
      <button type="button" className="secondary-button" disabled={pending || !pageFresh || history.offset === 0} onClick={() => void readPage(history.projectId, Math.max(0, history.offset - pageSize), history.revision)}>上一页</button>
      <p className="secondary">共 {history.total} 条{history.total > 0 ? `，当前 ${history.offset + 1}–${history.offset + decisions.length} 条` : ""}</p>
      <button type="button" className="secondary-button" disabled={pending || !pageFresh || history.nextOffset === null} onClick={() => { if (history.nextOffset !== null) void readPage(history.projectId, history.nextOffset, history.revision); }}>下一页</button>
    </nav>}
    {review && <dialog ref={dialog} className="panel modal management-dialog" aria-labelledby="management-review-title" aria-describedby="management-review-impact" onCancel={e => { e.preventDefault(); if (!pending) setReview(null); }}>
      <HelpHeading as="h2" topic="ManagementReview" id="management-review-title">{review.action === "close" ? "确认关闭未提交应用" : review.action === "discard" ? "确认放弃本地指挥请求" : "确认保存指挥答复"}</HelpHeading>
      <dl className="management-identifiers"><dt>项目</dt><dd>{review.projectRoot}</dd><dt>请求</dt><dd><code>{review.decision.operationId}</code></dd></dl>
      {review.action === "close"
        ? <>{review.decision.applicationRecovery && <ApplicationTarget candidate={review.decision.applicationRecovery}/>}<p id="management-review-impact">关闭只保存本地处置记录，不会发送编程任务、执行这条应用指令或自动恢复项目调度。原始指挥记录继续保留，已记录的请求额度不会退回。</p></>
        : review.action === "discard"
        ? <p id="management-review-impact">此操作仅关闭本地无工具指挥请求，不证明供应商已经取消处理或退费，也不会重发请求。它不会取消已经派发的编程任务。</p>
        : <><div className="management-saved-text"><HelpHeading as="h3" topic="ManagementReview">指挥等待原因（原文）</HelpHeading><p className="management-text">{review.decision.waitReason}</p></div><p id="management-review-impact">答复会持久保存，供下一次指挥请求参考。它不改变已经批准的执行范围，也不会自动恢复调度。</p></>}
      <form onSubmit={e => { e.preventDefault(); void confirm(); }}>
        <label htmlFor="management-review-input">{review.action === "close" ? "关闭原因" : review.action === "discard" ? "放弃理由" : "给指挥的答复"}</label>
        <textarea id="management-review-input" rows={5} autoFocus required value={input} disabled={pending} aria-describedby="management-input-limit" onChange={e => setInput(e.target.value)}/>
        <p className="secondary" id="management-input-limit">{inputBytes} / {maxInputBytes} 字节（UTF-8）</p>
        {inputBytes > maxInputBytes && <p role="alert">内容过长，请缩短后再提交。</p>}{hasNul && <p role="alert">内容包含不可保存的 NUL 字符，请删除后再提交。</p>}
        {!current && <p role="alert">这条请求或项目状态已变化，当前确认已失效。请返回并刷新列表后重新审阅。</p>}
        {error && <p role="alert"><ErrorText error={error}/></p>}
        <div className="modal-actions"><button type="button" className="secondary-button" disabled={pending} onClick={() => setReview(null)}>返回</button><button type="submit" className="primary" disabled={pending || !current || !text || hasNul || inputBytes > maxInputBytes}>{submitting ? "正在保存…" : review.action === "close" ? "确认关闭未提交应用" : review.action === "discard" ? "确认放弃本地请求" : "确认保存答复"}</button></div>
      </form>
    </dialog>}
  </section>;
}
