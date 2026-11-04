import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { useEffect, useRef, useState } from "react";
import type { CodexStartupSummary, CodexStartupView, Command, Snapshot } from "./contracts";
import "./CodexStartups.css";

const labels: Record<string, string> = {
  prepared: "启动准备已记录", advanced: "启动已进入后续流程",
  closed: "启动记录已关闭", legacy: "旧记录缺少启动证据",
};
const maxReasonBytes = 8000;
function sameRecord(a: CodexStartupSummary, b: CodexStartupSummary) {
  return a.projectId === b.projectId && a.taskId === b.taskId && a.attemptId === b.attemptId
    && a.managementOperation === b.managementOperation && a.state === b.state && a.intentHash === b.intentHash;
}
function StartupFields({ detail, compact = false }: { detail: CodexStartupView; compact?: boolean }) {
  const item = detail.summary;
  return <dl className={`startup-fields${compact ? " startup-fields-compact" : ""}`}>
    <dt>项目</dt><dd>{item.projectId}</dd><dt>任务</dt><dd>{item.taskId}</dd>
    <dt>尝试</dt><dd><code>{item.attemptId}</code></dd>
    <dt>关联指挥请求</dt><dd><code>{item.managementOperation ?? "未关联指挥请求"}</code></dd>
    <dt>工作目录</dt><dd>{detail.workspace}</dd>
    {!compact && <><dt>编程模型</dt><dd>{detail.workerModel}</dd>
    <dt>启动记录时间</dt><dd>{detail.startedAt}</dd><dt>启动协议</dt><dd>{detail.protocol ?? "未记录"}</dd>
    <dt>批准方案摘要</dt><dd><code>{detail.revisionDigest}</code></dd>
    <dt>工作流记录序号</dt><dd>{detail.workflowSequence}</dd></>}
  </dl>;
}

export function CodexStartups({ data, send, query, busy }: {
  data: Snapshot | null;
  send: (command: Command) => Promise<boolean>;
  query: (command: Command) => Promise<Snapshot | null>;
  busy: boolean;
}) {
  const [detail, setDetail] = useState<CodexStartupView | null>(null);
  const [review, setReview] = useState<CodexStartupView | null>(null);
  const [reason, setReason] = useState("");
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const inFlight = useRef(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const panel = useRef<HTMLElement>(null);
  const rows = data?.codexStartups ?? [];
  const matching = (value: CodexStartupView) => rows.find(item => item.projectId === value.summary.projectId && item.attemptId === value.summary.attemptId);
  const detailRow = detail && matching(detail);
  const detailFresh = !!detail && !!detailRow && sameRecord(detail.summary, detailRow)
    && data?.workflows.find(w => w.project.id === detail.summary.projectId)?.sequence === detail.workflowSequence;
  const reviewRow = review && matching(review);
  const current = !!review && !!reviewRow && !!review.evidenceDigest && sameRecord(review.summary, reviewRow)
    && detail?.evidenceDigest === review.evidenceDigest
    && data?.workflows.find(w => w.project.id === review.summary.projectId)?.sequence === review.workflowSequence;
  const pending = busy || loading || saving;
  const text = reason.trim();
  const bytes = new TextEncoder().encode(text).length;
  const hasNul = text.includes("\0");

  useEffect(() => {
    if (detail && !review && !loading && !saving) panel.current?.scrollIntoView({ block: "start" });
  }, [detail, review, loading, saving]);
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

  async function read(item: CodexStartupSummary, forReview?: CodexStartupView) {
    if (pending || inFlight.current) return;
    inFlight.current = true; setLoading(true); setError(null); setNotice(null);
    try {
      const response = await query({ type: "readCodexStartup", projectId: item.projectId, attemptId: item.attemptId });
      const next = response?.codexStartupDetail;
      if (!next || next.summary.projectId !== item.projectId || next.summary.attemptId !== item.attemptId) {
        setError("启动详情未能读取，请重新查看记录。"); return;
      }
      setDetail(next);
      if (!forReview) return;
      if (!sameRecord(item, next.summary) || !next.evidenceDigest || next.evidenceDigest !== forReview.evidenceDigest) {
        setError("启动证据或项目状态已变化，请审阅更新后的详情再操作。"); return;
      }
      setReason(""); setReview(next);
    } catch {
      setError("启动详情未能读取，未打开关闭确认。");
    } finally {
      inFlight.current = false; setLoading(false);
    }
  }

  async function confirm() {
    if (!review || !current || !review.evidenceDigest || pending || inFlight.current || !text || hasNul || bytes > maxReasonBytes) return;
    inFlight.current = true; setSaving(true); setError(null); setNotice(null);
    const item = review.summary;
    try {
      const ok = await send({ type: "abandonCodexStartup", projectId: item.projectId, attemptId: item.attemptId, evidenceDigest: review.evidenceDigest, reason: text });
      if (!ok) { setError("关闭未完成，请核对当前启动状态；填写的原因已保留。"); return; }
      setReview(null);
      const response = await query({ type: "readCodexStartup", projectId: item.projectId, attemptId: item.attemptId });
      const next = response?.codexStartupDetail;
      if (next?.summary.projectId === item.projectId && next.summary.attemptId === item.attemptId) {
        setDetail(next); setNotice("启动记录已关闭，原始记录继续保留。项目调度不会自动恢复。");
      } else { setDetail(null); setError("关闭已保存，但最新详情未能读取，请重新查看记录。"); }
    } catch {
      setError("未能确认最新关闭状态，请重新读取记录；系统不会自动重发任务。");
    } finally {
      inFlight.current = false; setSaving(false);
    }
  }

  if (!rows.length && !detail && !review) return null;
  return <section className="workflow-plans codex-startups" aria-label="编程启动记录">
    <HelpHeading as="h2" topic="CodexStartups">编程启动记录</HelpHeading>
    <p className="secondary">查看启动准备及后续处置。没有编程执行记录的启动意图也会保留在这里；关闭须先读取完整证据。</p>
    {error && <p role="alert" className="startup-feedback"><ErrorText error={error}/></p>}
    {notice && <p role="status" className="startup-feedback">{notice}</p>}
    {loading && <p role="status" className="secondary">正在读取启动详情…</p>}
    {detail && <article ref={panel} className="panel startup-detail" aria-label="编程启动详情" data-startup-detail={detail.summary.attemptId} data-startup-project={detail.summary.projectId}>
      <div className="section-heading"><HelpHeading as="h3" topic="CodexStartups">编程启动详情</HelpHeading><button type="button" className="secondary-button" disabled={pending} onClick={() => setDetail(null)}>收起启动详情</button></div>
      <p><span className="badge">{labels[detail.summary.state] ?? detail.summary.state}</span></p>
      <StartupFields detail={detail}/>
      {!detailFresh && <p role="status" className="startup-stale">记录已变化或不在当前列表，请重新读取详情后再审阅。</p>}
      {detail.reason !== null && <div className="startup-reason"><HelpHeading as="h4" topic="CodexStartups">关闭原因</HelpHeading><p>{detail.reason}</p></div>}
      {detail.blockedReason !== null && <p className="startup-text">{detail.blockedReason}</p>}
      {detail.summary.state === "legacy" && <p className="secondary">旧记录缺少可验证的启动意图，只能查看现有证据，不能从此入口关闭。</p>}
      {detail.evidenceDigest !== null && detail.summary.state !== "legacy" && <><p className="secondary">可能已经建立远端会话。此处只能关闭本地启动准备，不代表远程会话已取消。</p><button type="button" className="secondary-button" disabled={pending || !detailFresh} onClick={() => void read(detail.summary, detail)}>关闭启动记录…</button></>}
    </article>}
    {rows.map(item => <article className="panel startup-row" key={`${item.projectId}:${item.attemptId}`} data-startup-attempt={item.attemptId} data-startup-project={item.projectId}>
      <div className="section-heading"><HelpHeading as="h3" topic="CodexStartups">任务 {item.taskId}</HelpHeading><span className="badge">{labels[item.state] ?? item.state}</span></div>
      <p className="secondary">项目 {item.projectId} · 尝试 <code>{item.attemptId}</code></p>
      <p className="secondary">关联指挥请求：{item.managementOperation ?? "未关联"}</p>
      {item.state === "legacy" && <p className="secondary">缺少启动证据，暂不可关闭。</p>}
      <button type="button" className="secondary-button" disabled={pending} onClick={() => void read(item)}>查看启动详情</button>
    </article>)}
    {review && <dialog ref={dialog} className="panel modal startup-dialog" aria-labelledby="startup-review-title" aria-describedby="startup-review-impact" onCancel={event => { event.preventDefault(); if (!pending) setReview(null); }}>
      <HelpHeading as="h2" topic="CodexStartups" id="startup-review-title">确认关闭编程启动记录</HelpHeading>
      <StartupFields detail={review} compact/>
      <p id="startup-review-impact">仅保存本地关闭处置，保留原始指挥请求和启动记录；不推进任务进度、不退回请求额度、不自动恢复调度或发送任务。此前可能已经建立远端会话，这不证明远程取消或退费。</p>
      <form onSubmit={event => { event.preventDefault(); void confirm(); }}>
        <label htmlFor="startup-close-reason">启动记录关闭原因</label><textarea id="startup-close-reason" rows={4} autoFocus required disabled={pending} value={reason} aria-describedby="startup-reason-limit" onChange={event => setReason(event.target.value)}/>
        <p id="startup-reason-limit" className="secondary">{bytes} / {maxReasonBytes} 字节（UTF-8）</p>
        {hasNul && <p role="alert">原因包含不可保存的 NUL 字符，请删除后再提交。</p>}
        {bytes > maxReasonBytes && <p role="alert">原因过长，请缩短后再提交。</p>}
        {!current && <p role="alert">启动记录或项目证据已变化，当前确认已失效。请返回重新读取详情。</p>}
        {error && <p role="alert"><ErrorText error={error}/></p>}
        <div className="modal-actions"><button type="button" className="secondary-button" disabled={pending} onClick={() => setReview(null)}>返回</button><button type="submit" className="primary" disabled={pending || !current || !text || hasNul || bytes > maxReasonBytes}>{saving ? "正在保存…" : "确认关闭启动记录"}</button></div>
      </form>
    </dialog>}
  </section>;
}
