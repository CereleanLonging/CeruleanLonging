import { useEffect, useRef, useState } from "react";
import type {
  CodexJob,
  CodexJobState,
  Command,
  ExecutionSnapshot,
  Snapshot,
  UnsentAssignmentReview,
} from "./contracts";
import "./CodexJobs.css";

const labels: Record<CodexJobState, string> = {
  starting: "正在连接执行器",
  executing: "编程执行中",
  reconciling: "正在核对原会话",
  submitted: "已提交，待最终验收",
  unknown: "执行结果待对账",
  failed: "执行启动失败",
  abandoned: "本次尝试已关闭",
  "cancel-requested": "已请求停止，等待执行器处理",
  cancelling: "正在等待停止结果",
  cancelled: "本次执行已停止",
};
const maxReasonBytes = 8000;
type CancellationReview = Snapshot["codexCancellations"][number];
type AttemptIdentity = { projectId: string; taskId: string; attemptId: string };
type Review =
  | { kind: "unsent"; item: UnsentAssignmentReview }
  | {
      kind: "cancel";
      item: CancellationReview;
      goal: string;
      root: string;
      projectName: string;
    };
function sameAttempt(left: AttemptIdentity, right: AttemptIdentity) {
  return (
    left.projectId === right.projectId &&
    left.taskId === right.taskId &&
    left.attemptId === right.attemptId
  );
}

export function CodexJobs({
  jobs,
  workflows = [],
  unsentAssignments = [],
  cancellations = [],
  send,
  busy,
}: {
  jobs: CodexJob[];
  workflows?: ExecutionSnapshot[];
  unsentAssignments?: UnsentAssignmentReview[];
  cancellations?: CancellationReview[];
  send: (command: Command) => Promise<boolean>;
  busy: boolean;
}) {
  const [review, setReview] = useState<Review | null>(null);
  const [reason, setReason] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const inFlight = useRef(false);
  const latest =
    review &&
    (review.kind === "cancel" ? cancellations : unsentAssignments).find(
      (item) => sameAttempt(item, review.item),
    );
  const current =
    !!review &&
    !!latest &&
    latest.evidenceDigest === review.item.evidenceDigest;
  const stopping = review?.kind === "cancel";
  const text = reason.trim();
  const bytes = new TextEncoder().encode(text).length;
  const hasNul = text.includes("\0");
  const pending = busy || saving;

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

  function open(item: UnsentAssignmentReview) {
    if (pending) return;
    setReason("");
    setError(null);
    setNotice(null);
    setReview({ kind: "unsent", item });
  }
  function target(item: AttemptIdentity, job?: CodexJob) {
    const workflow = workflows.find(
      (value) => value.project.id === item.projectId,
    );
    const root = workflow?.project.root ?? "项目目录暂不可用";
    const matchingRevision =
      !job || job.revisionDigest === workflow?.revisionDigest;
    const goal = matchingRevision
      ? workflow?.revision?.workflow.tasks.find(
          (task) => task.id === item.taskId,
        )?.goal
      : undefined;
    return {
      root,
      projectName:
        workflow?.project.root.split(/[\\/]/).filter(Boolean).at(-1) ??
        "编程项目",
      goal: goal ?? (matchingRevision ? "编程任务" : "历史编程任务"),
    };
  }
  function openCancellation(item: CancellationReview, job: CodexJob) {
    if (pending) return;
    setReason("");
    setError(null);
    setNotice(null);
    setReview({ kind: "cancel", item, ...target(item, job) });
  }
  async function confirm() {
    if (
      !review ||
      !current ||
      pending ||
      inFlight.current ||
      !text ||
      bytes > maxReasonBytes ||
      hasNul
    )
      return;
    inFlight.current = true;
    setSaving(true);
    setError(null);
    try {
      const command: Command =
        review.kind === "cancel"
          ? {
              type: "cancelCodex",
              projectId: review.item.projectId,
              attemptId: review.item.attemptId,
              evidenceDigest: review.item.evidenceDigest,
              reason: text,
            }
          : { type: "abandonUnsentAttempt", ...review.item, reason: text };
      if (await send(command)) {
        if (review.kind === "cancel")
          setNotice(
            "停止请求已提交，后续调度已暂停。请查看任务的最新执行结果；提交请求不代表执行已经停止。",
          );
        setReview(null);
      } else
        setError(
          review.kind === "cancel"
            ? "未能确认停止请求已受理，请核对最新状态；填写的原因已保留。"
            : "放弃未完成，请核对当前尝试状态；填写的原因已保留。",
        );
    } catch {
      setError("未能确认处置结果，请重新读取记录。系统不会自动重发编程任务。");
    } finally {
      inFlight.current = false;
      setSaving(false);
    }
  }
  function unsentAction(item: UnsentAssignmentReview) {
    return (
      <div className="unsent-assignment-action">
        <p>
          核心记录显示该尝试已分配，尚未记录派发请求。放弃后任务回到待分配，不会自动恢复调度或重新派发。
        </p>
        <button
          type="button"
          className="secondary-button"
          disabled={pending}
          onClick={() => open(item)}
        >
          放弃未派发尝试…
        </button>
      </div>
    );
  }

  if (!jobs.length && !unsentAssignments.length && !review) return null;
  return (
    <section className="workflow-plans codex-jobs" aria-label="编程执行记录">
      <h2>编程执行</h2>
      {notice && (
        <p role="status" className="codex-request-notice">
          {notice}
        </p>
      )}
      {jobs.map((job) => {
        const unsent = unsentAssignments.find((item) => sameAttempt(item, job));
        const cancellation = cancellations.find((item) =>
          sameAttempt(item, job),
        );
        const task = workflows.find(
          (workflow) => workflow.project.id === job.projectId,
        )?.state.tasks[job.taskId];
        const awaitingCancellation =
          task?.attempt === job.attemptId &&
          task.cancellationRequested === true;
        const canReconcile =
          job.state === "unknown" &&
          task?.state === "unknown" &&
          task.attempt === job.attemptId;
        const description = target(job, job);
        return (
          <article
            className="panel codex-job"
            key={`${job.projectId}:${job.attemptId}`}
            data-codex-attempt={job.attemptId}
            data-codex-project={job.projectId}
          >
            <div className="section-heading">
              <h3>{description.goal}</h3>
              <span className="badge">
                {unsent
                  ? "已分配但尚未派发"
                  : job.state === "unknown" && awaitingCancellation
                    ? "停止结果待核对"
                    : labels[job.state]}
              </span>
            </div>
            <p className="secondary">
              {job.worker.model} · {description.projectName} ·{" "}
              {new Date(job.updatedAt).toLocaleString("zh-CN")}
            </p>
            <details className="codex-record-identity">
              <summary>记录详情</summary>
              <p>{description.root}</p>
              <p>
                项目：<code>{job.projectId}</code> · 任务：
                <code>{job.taskId}</code>
              </p>
              <p>
                尝试：<code>{job.attemptId}</code>
              </p>
            </details>
            {job.message && <p>{job.message}</p>}
            {job.state === "cancel-requested" && (
              <p className="codex-cancellation-status" role="status">
                停止请求已排队，执行器尚未确认处理。后续调度已暂停，请等待最新结果。
              </p>
            )}
            {job.state === "cancelling" && (
              <p className="codex-cancellation-status" role="status">
                停止请求已记录，正在等待执行器的最终结果。当前还不能确认任务已经停止。
              </p>
            )}
            {job.state === "cancelled" && (
              <p className="codex-cancellation-status">
                执行器已确认本次执行停止，已有修改和记录继续保留。后续工作需要重新规划并批准。
              </p>
            )}
            {job.state === "submitted" && (
              <p className="secondary">执行结果已保留，待最终验收。</p>
            )}
            {job.state === "unknown" && awaitingCancellation && (
              <p className="codex-cancellation-status">
                停止结果尚未确认。请先核对原会话；系统不会自动重发任务或恢复调度。
              </p>
            )}
            {!unsent && cancellation && (
              <div className="codex-cancellation-action">
                <p>
                  将暂停本项目后续调度，并请求执行器停止当前任务。已有修改会保留。
                </p>
                <button
                  type="button"
                  className="secondary-button"
                  disabled={pending}
                  onClick={() => openCancellation(cancellation, job)}
                >
                  请求停止当前任务…
                </button>
              </div>
            )}
            {unsent ? (
              unsentAction(unsent)
            ) : canReconcile ? (
              <>
                <p>核对会读取原会话，系统不会重新发送编程任务。</p>
                <button
                  type="button"
                  className="secondary-button"
                  disabled={pending}
                  onClick={() =>
                    void send({
                      type: "reconcileCodex",
                      projectId: job.projectId,
                      attemptId: job.attemptId,
                    })
                  }
                >
                  核对原会话
                </button>
              </>
            ) : (
              job.state === "unknown" && (
                <p className="secondary">
                  当前尝试与待对账工作流不匹配。请先检查主副指挥请求记录或恢复工作流账本。
                </p>
              )
            )}
            {job.state === "abandoned" && (
              <p className="secondary">
                本次尝试已关闭，原记录保留。后续执行需要手动恢复项目调度。
              </p>
            )}
          </article>
        );
      })}
      {unsentAssignments
        .filter((item) => !jobs.some((job) => sameAttempt(item, job)))
        .map((item) => (
          <article
            className="panel codex-job"
            key={`${item.projectId}:${item.attemptId}`}
            data-codex-attempt={item.attemptId}
            data-codex-project={item.projectId}
          >
            <div className="section-heading">
              <h3>任务 {item.taskId}</h3>
              <span className="badge">已分配但尚未派发</span>
            </div>
            <p className="secondary">
              项目 {item.projectId} · 尝试 <code>{item.attemptId}</code>
            </p>
            {unsentAction(item)}
          </article>
        ))}
      {review && (
        <dialog
          ref={dialog}
          className={`panel modal unsent-assignment-dialog${stopping ? " codex-cancellation-dialog" : ""}`}
          aria-labelledby="unsent-assignment-title"
          aria-describedby="unsent-assignment-impact"
          onCancel={(event) => {
            event.preventDefault();
            if (!pending) setReview(null);
          }}
        >
          <h2 id="unsent-assignment-title">
            {stopping ? "确认请求停止当前任务" : "确认放弃未派发尝试"}
          </h2>
          {review.kind === "cancel" ? (
            <>
              <dl className="codex-cancellation-target">
                <dt>项目</dt>
                <dd>{review.projectName}</dd>
                <dt>任务目标</dt>
                <dd>{review.goal}</dd>
                <dt>项目目录</dt>
                <dd>{review.root}</dd>
              </dl>
              <details className="codex-record-identity">
                <summary>记录标识</summary>
                <dl>
                  <dt>项目标识</dt>
                  <dd>
                    <code>{review.item.projectId}</code>
                  </dd>
                  <dt>任务标识</dt>
                  <dd>
                    <code>{review.item.taskId}</code>
                  </dd>
                  <dt>尝试标识</dt>
                  <dd>
                    <code>{review.item.attemptId}</code>
                  </dd>
                </dl>
              </details>
            </>
          ) : (
            <dl>
              <dt>项目</dt>
              <dd>{review.item.projectId}</dd>
              <dt>任务</dt>
              <dd>{review.item.taskId}</dd>
              <dt>尝试</dt>
              <dd>
                <code>{review.item.attemptId}</code>
              </dd>
            </dl>
          )}
          <p id="unsent-assignment-impact">
            {stopping
              ? "先暂停本项目后续调度，再请求执行器停止当前任务。提交请求不代表已经停止；如果任务已先完成，将保留结果等待验收。已有文件修改不会回滚，系统不会自动重新派发。"
              : "这会关闭当前尚未派发的分配记录，使任务回到待分配状态。不会自动恢复调度或重新发送任务；后续执行需要手动恢复项目调度。"}
          </p>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void confirm();
            }}
          >
            <label htmlFor="unsent-assignment-reason">
              {stopping ? "停止原因" : "放弃未派发尝试的原因"}
            </label>
            <textarea
              id="unsent-assignment-reason"
              rows={4}
              value={reason}
              required
              autoFocus
              disabled={pending}
              aria-describedby="unsent-assignment-limit"
              onChange={(event) => setReason(event.target.value)}
            />
            <p id="unsent-assignment-limit" className="secondary">
              {bytes} / {maxReasonBytes} 字节（UTF-8）
            </p>
            {bytes > maxReasonBytes && (
              <p role="alert">原因过长，请缩短后再提交。</p>
            )}
            {hasNul && (
              <p role="alert">原因包含不可保存的 NUL 字符，请删除后再提交。</p>
            )}
            {!current && (
              <p role="alert">
                {stopping
                  ? "执行状态已变化或不再可请求停止。请返回查看最新结果并重新审阅。"
                  : "尝试记录已变化或不再可放弃。请返回查看最新状态并重新审阅。"}
              </p>
            )}
            {error && <p role="alert">{error}</p>}
            <div className="modal-actions">
              <button
                type="button"
                className="secondary-button"
                disabled={pending}
                onClick={() => setReview(null)}
              >
                返回检查
              </button>
              <button
                type="submit"
                className="primary"
                disabled={
                  pending ||
                  !current ||
                  !text ||
                  bytes > maxReasonBytes ||
                  hasNul
                }
              >
                {saving
                  ? "正在保存…"
                  : stopping
                    ? "确认请求停止"
                    : "确认放弃未派发尝试"}
              </button>
            </div>
          </form>
        </dialog>
      )}
    </section>
  );
}
