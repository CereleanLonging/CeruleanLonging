import { useEffect, useRef, useState } from "react";
import type {
  Command,
  Snapshot,
  OmpFilesView,
  OmpFileReview,
  FilePreview,
} from "./contracts";
import "./OmpFileRestore.css";
const status: Record<string, string> = {
  original: "与原文件一致",
  intended: "与本次写入目标一致",
  other: "内容已变化，需核对",
  missing: "文件已缺失",
  unavailable: "证据不可用",
};
const restored: Record<string, string> = {
  restored: "文件已还原，任务仍待核对。",
  "changed-after-restore": "还原后文件又有变化，请核对当前内容。",
  "restore-interrupted": "上次还原未确认完成，请保留证据并进一步处理。",
  "restore-unavailable": "还原记录不完整，当前不可再次执行。",
};
function Preview({ label, value }: { label: string; value: FilePreview }) {
  return (
    <section className="file-preview">
      <h4>{label}</h4>
      <p>
        {value.bytes === null ? "文件不存在" : `${value.bytes} 字节`}
        {value.truncated ? " · 预览已截断" : ""}
      </p>
      <pre>
        {value.text ??
          (value.binary ? "二进制内容，不提供文本预览" : "无文件内容")}
      </pre>
    </section>
  );
}
export function OmpFileRestore({
  data,
  query,
  busy,
}: {
  data: Snapshot | null;
  query: (command: Command) => Promise<Snapshot | null>;
  busy: boolean;
}) {
  const [view, setView] = useState<OmpFilesView | null>(null);
  const [review, setReview] = useState<OmpFileReview | null>(null);
  const [ack, setAck] = useState(false);
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState("");
  const inFlight = useRef(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const candidates = (data?.workflows ?? []).flatMap((w) =>
    Object.entries(w.state.tasks).flatMap(([taskId, t]) =>
      t?.state === "unknown" &&
      t.attempt &&
      w.revision?.execution.some(
        (e) => e.taskId === taskId && e.adapterId === "omp",
      )
        ? [{ projectId: w.project.id, taskId, attemptId: t.attempt }]
        : [],
    ),
  );
  const detail = view?.detail;
  const blocked = busy || pending;
  const current =
    review &&
    detail?.evidenceDigest === review.evidenceDigest &&
    data?.workflows.some(
      (w) =>
        w.project.id === review.projectId &&
        w.sequence === review.workflowSequence &&
        w.state.tasks[review.taskId]?.state === "unknown" &&
        w.state.tasks[review.taskId]?.attempt === review.attemptId,
    );
  useEffect(() => {
    if (!review || !dialog.current) return;
    const opener = document.activeElement;
    dialog.current.showModal();
    return () => {
      dialog.current?.close();
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, [review]);
  async function read(
    scope: { projectId: string; taskId: string; attemptId: string },
    callId: string | null = null,
    offset = 0,
  ) {
    if (blocked || inFlight.current) return;
    inFlight.current = true;
    setPending(true);
    setMessage("");
    try {
      const next = await query({
        type: "readOmpFiles",
        ...scope,
        offset,
        callId,
      });
      const value = next?.ompFiles;
      if (
        !value ||
        value.projectId !== scope.projectId ||
        value.taskId !== scope.taskId ||
        value.attemptId !== scope.attemptId ||
        (callId !== null && value.detail?.callId !== callId)
      ) {
        setMessage("读取失败，请重新核对文件记录。");
        return;
      }
      setView(value);
    } finally {
      inFlight.current = false;
      setPending(false);
    }
  }
  async function confirm() {
    if (
      !review ||
      !current ||
      !ack ||
      !review.evidenceDigest ||
      blocked ||
      inFlight.current
    )
      return;
    inFlight.current = true;
    setPending(true);
    setMessage("");
    try {
      const next = await query({
        type: "restoreOmpFile",
        projectId: review.projectId,
        taskId: review.taskId,
        attemptId: review.attemptId,
        callId: review.callId,
        evidenceDigest: review.evidenceDigest,
      });
      setReview(null);
      setAck(false);
      const outcome = next?.ompRestore;
      const confirmed = outcome &&
        outcome.projectId === review.projectId &&
        outcome.taskId === review.taskId &&
        outcome.attemptId === review.attemptId &&
        outcome.callId === review.callId &&
        outcome.reviewDigest === review.evidenceDigest;
      if (confirmed) {
        const value = next?.ompFiles;
        const previewMatches = !outcome.readbackError && value &&
          value.projectId === review.projectId &&
          value.taskId === review.taskId &&
          value.attemptId === review.attemptId &&
          value.detail?.callId === review.callId;
        setView(previewMatches ? value : null);
        setMessage(previewMatches
          ? "文件还原已记录。任务仍保持待核对状态。"
          : "文件还原已完成并记录，但当前文件预览暂不可用。请重新读取记录；不要重复还原。任务仍待核对。");
      } else {
        setView((v) => (v ? { ...v, detail: null } : null));
        setMessage("还原未确认完成。请重新读取记录，核对当前文件和还原结果。");
      }
    } finally {
      inFlight.current = false;
      setPending(false);
    }
  }
  return (
    <section
      className="panel omp-file-restore"
      aria-labelledby="omp-files-heading"
    >
      <h2 id="omp-files-heading">OMP 文件恢复</h2>
      <p>核对未完成任务的文件改动，确认后恢复到本次写入前。</p>
      {!candidates.length && (
        <p className="muted">当前没有待核对的 OMP 尝试。</p>
      )}
      <div className="file-attempts">
        {candidates.map((c) => (
          <button
            key={`${c.projectId}/${c.attemptId}`}
            disabled={blocked}
            onClick={() => void read(c)}
          >
            查看文件记录 · {c.projectId} / {c.taskId}
          </button>
        ))}
      </div>
      {view && (
        <>
          <p>
            项目 {view.projectId} · 任务 {view.taskId} · 共 {view.total}{" "}
            条写入记录
          </p>
          <ul>
            {view.files.map((item) => (
              <li key={item.callId}>
                <span>
                  {item.path ?? "未能读取文件名"} ·{" "}
                  {status[item.state] ?? item.state}
                </span>
                <button
                  disabled={blocked || !!item.error}
                  onClick={() => void read(view, item.callId, view.offset)}
                >
                  核对文件
                </button>
              </li>
            ))}
          </ul>
          <div className="file-attempts">
            <button
              disabled={blocked || view.offset === 0}
              onClick={() =>
                void read(view, null, Math.max(0, view.offset - 16))
              }
            >
              上一页
            </button>
            <button
              disabled={blocked || view.offset + 16 >= view.total}
              onClick={() => void read(view, null, view.offset + 16)}
            >
              下一页
            </button>
          </div>
        </>
      )}
      {detail && (
        <article className="file-detail">
          <h3>{detail.path}</h3>
          <p>{status[detail.state] ?? detail.state}</p>
          <div className="file-preview-grid">
            <Preview label="当前文件" value={detail.current} />
            <Preview label="还原后的文件" value={detail.original} />
          </div>
          <details>
            <summary>文件版本摘要</summary>
            <p>
              当前：<code>{detail.currentSha256 ?? "不存在"}</code>
            </p>
            <p>
              原文：<code>{detail.originalSha256 ?? "不存在"}</code>
            </p>
          </details>
          {detail.restoreStatus ? (
            <p role="status">{restored[detail.restoreStatus]}</p>
          ) : (
            <button
              disabled={blocked || !detail.evidenceDigest}
              onClick={() => {
                setAck(false);
                setReview(detail);
              }}
            >
              复核并还原
            </button>
          )}
        </article>
      )}
      {message && <p role="status">{message}</p>}
      {review && (
        <dialog
          ref={dialog}
          aria-labelledby="file-restore-title"
          onCancel={(e) => {
            if (pending) e.preventDefault();
            else setReview(null);
          }}
        >
          <h3 id="file-restore-title">确认文件还原</h3>
          <p>
            {review.projectId} / {review.taskId} · {review.path}
          </p>
          <p>
            {review.originalSha256 === null
              ? "此文件原先不存在，确认后将删除本次新增文件。"
              : "确认后将用本次写入前的内容替换当前文件。"}
            任务仍需继续核对。
          </p>
          <div className="file-preview-grid">
            <Preview label="当前文件" value={review.current} />
            <Preview label="还原后的文件" value={review.original} />
          </div>
          <label>
            <input
              type="checkbox"
              checked={ack}
              disabled={pending}
              onChange={(e) => setAck(e.target.checked)}
            />
            我已核对文件内容，确认执行上述还原
          </label>
          {!current && <p role="alert">记录已变化，请关闭并重新读取。</p>}
          <div className="file-attempts">
            <button disabled={pending} onClick={() => setReview(null)}>
              取消
            </button>
            <button
              disabled={blocked || !ack || !current}
              onClick={() => void confirm()}
            >
              确认还原文件
            </button>
          </div>
        </dialog>
      )}
    </section>
  );
}
