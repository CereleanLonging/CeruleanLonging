import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ErrorText } from "./ErrorText";
import type { ReviewSessionProgress } from "./contracts";

type View = {
  projectId: string; root: string; total: number; completed: number;
  runs: { adapter: string; task: string; attempt: string; model: string; state: string; message: string | null }[];
  reviewProgress: ReviewSessionProgress | null;
};
const states: Record<string, string> = { starting: "正在建立会话", executing: "执行中", submitted: "已提交", cancelled: "已取消", failed: "执行失败", unknown: "等待核对", reconciling: "核对原记录", "cancel-requested": "正在停止", cancelling: "正在停止" };
const nativeRead = () => invoke<View>("cli_session_read");
export function CliSessionWindow({ read = nativeRead }: { read?: () => Promise<View> }) {
  const [data, setData] = useState<View | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let current = true;
    let timer: ReturnType<typeof setTimeout>;
    async function refresh() {
      try { const value = await read(); if (current) { setData(value); setError(""); } }
      catch (reason) { if (current) setError(String(reason)); }
      if (current) timer = setTimeout(() => void refresh(), 2000);
    }
    void refresh();
    return () => { current = false; clearTimeout(timer); };
  }, [read]);
  const review = data?.reviewProgress;
  return <main className="review-logs-window cli-session-window">
    <header><div><div className="eyebrow">PROJECT SESSION</div><h1>项目进度与 CLI 会话</h1></div><span className="pill">核心关联 · 只读</span></header>
    {error && <p role="alert"><ErrorText error={error}/></p>}
    {!data && !error && <p role="status">正在读取所属工作台的项目进度…</p>}
    {data && <>
      <section className="panel"><h2>{data.projectId}</h2><p><code>{data.root}</code></p><p>已接受 {data.completed} / {data.total} 个任务</p><progress value={data.completed} max={Math.max(1, data.total)}/></section>
      <section className="panel"><h2>执行会话进度</h2>
        {data.runs.length ? data.runs.map(run => <article key={`${run.adapter}-${run.attempt}`}><h3>{run.adapter} · {run.model}</h3><p>{run.task} · {states[run.state] ?? "等待核心核对"}</p><p>尝试 <code>{run.attempt}</code></p>{run.message && <p>{run.message}</p>}</article>) : <p>当前项目尚无 CLI 执行记录。</p>}
      </section>
      <section className="panel" aria-label="分片材料会话"><h2>分片材料与会话</h2>
        {review ? <><p>{review.seat} · {review.model}</p><p>会话 ID <code>{review.sessionId ?? "正在建立"}</code></p><p>消息 {review.packet} / {review.packets} · {review.acknowledged ? "已核对回执" : "等待原会话回执"}</p><pre className="review-session-material">{review.materialMessage}</pre></> : <p>核心转发评审材料时，这里显示对应会话和当前分片。投票收齐前保持密封。</p>}
      </section>
    </>}
  </main>;
}
