import { useEffect, useRef, useState } from "react";
import type { Command, ManagementView, Snapshot } from "./contracts";

export function AdviceTracking({ decision, data, send, query, busy, refresh }: {
  decision: ManagementView; data: Snapshot | null; busy: boolean;
  send: (command: Command) => Promise<boolean>;
  query: (command: Command) => Promise<Snapshot | null>;
  refresh: () => Promise<void>;
}) {
  const [review, setReview] = useState<ManagementView | null>(null);
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const lock = useRef(false);
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    if (!review || !dialog.current) return;
    const opener = document.activeElement, modal = dialog.current;
    modal.showModal();
    return () => { modal.close(); if (opener instanceof HTMLElement && opener.isConnected) opener.focus(); };
  }, [review]);
  const candidate = review?.adviceTrackingReview;
  const current = !!candidate && review?.operationId === decision.operationId
    && candidate.evidenceDigest === decision.adviceTrackingReview?.evidenceDigest
    && data?.workflows.find(w => w.project.id === decision.projectId)?.sequence === candidate.workflowSequence
    && !data?.projectRuns.some(r => r.projectId === decision.projectId && r.state === "active");
  async function begin() {
    if (busy || lock.current) return;
    lock.current = true; setPending(true); setError(null);
    try {
      const next = await query({ type: "readManagementDetail", operationId: decision.operationId });
      const fresh = next?.managementDetail;
      if (!fresh?.adviceTrackingReview || fresh.operationId !== decision.operationId
        || fresh.adviceTrackingReview.evidenceDigest !== decision.adviceTrackingReview?.evidenceDigest) {
        setError("建议或项目状态已变化，请重新读取完整记录。"); return;
      }
      setReason(""); setReview(fresh);
    } catch { setError("未能读取最新证据，请重试。"); }
    finally { lock.current = false; setPending(false); }
  }
  async function confirm() {
    if (!current || busy || lock.current || !reason.trim() || reason.includes("\0") || new TextEncoder().encode(reason).length > 8000) return;
    lock.current = true; setPending(true); setError(null);
    try {
      const ok = await send({ type: "closeAdviceTracking", operationId: decision.operationId, evidenceDigest: candidate!.evidenceDigest, reason: reason.trim() });
      if (!ok) { setError("处置未确认，请核对最新状态；填写内容已保留。"); return; }
      setReview(null); await refresh();
    } catch { setError("未能确认处置结果，请重新读取记录；不会自动重发。"); }
    finally { lock.current = false; setPending(false); }
  }
  return <section aria-label="建议跟踪处置" className="management-saved-text">
    {decision.adviceTrackingClosure ? <>
      <h4>建议跟踪已由用户结束</h4>
      <p>送达仍未知。原建议和发送证据保留；此处置不代表接收、执行成功或验收通过。</p>
      <p className="management-text">{decision.adviceTrackingClosure.reason}</p>
      <p className="secondary">{decision.adviceTrackingClosure.at}</p>
    </> : decision.adviceTrackingReview ? <>
      <p>可以结束这条未知建议的跟踪。原执行结果仍须独立对账，项目需明确恢复后才继续。</p>
      <button type="button" className="secondary-button" disabled={busy || pending} onClick={() => void begin()}>结束建议跟踪…</button>
    </> : null}
    {error && <p role="alert">{error}</p>}
    {review && <dialog ref={dialog} className="panel modal management-dialog" aria-labelledby="advice-close-title" onCancel={e => { e.preventDefault(); if (!pending) setReview(null); }}>
      <h2 id="advice-close-title">确认结束建议跟踪</h2>
      <dl className="management-identifiers"><dt>项目</dt><dd>{data?.workflows.find(w => w.project.id === review.projectId)?.project.root ?? review.projectId}</dd><dt>请求</dt><dd><code>{review.operationId}</code></dd></dl>
      <p>任务 {review.supervisionAdvice?.taskId} · 尝试 {review.supervisionAdvice?.attemptId}</p>
      <p className="management-text">{review.supervisionAdvice?.suggestion}</p>
      <p>送达状态继续保留为未知，不会重发建议，也不代表远端取消或退费。此操作不改变任务结果或自动恢复调度。</p>
      <form onSubmit={e => { e.preventDefault(); void confirm(); }}>
        <label htmlFor="advice-close-reason">结束跟踪的原因</label>
        <textarea id="advice-close-reason" autoFocus required rows={4} value={reason} disabled={pending} onChange={e => setReason(e.target.value)} />
        <p className="secondary">{new TextEncoder().encode(reason).length} / 8000 字节</p>
        {!current && <p role="alert">项目状态已变化，请返回并重新审阅。</p>}
        {error && <p role="alert">{error}</p>}
        <div className="modal-actions"><button type="button" className="secondary-button" disabled={pending} onClick={() => setReview(null)}>返回</button><button type="submit" className="primary" disabled={pending || busy || !current || !reason.trim() || reason.includes("\0") || new TextEncoder().encode(reason).length > 8000}>确认结束建议跟踪</button></div>
      </form>
    </dialog>}
  </section>;
}
