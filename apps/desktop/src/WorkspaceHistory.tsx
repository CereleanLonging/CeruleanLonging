import { HelpHeading } from "./HelpHeading";
import { useEffect, useRef, useState } from "react";
import type { WorkspaceHistoryPage, WorkspaceHistoryDetail } from "./contracts";
import type { WorkspaceReader } from "./WorkspaceBrowser";
export function WorkspaceHistory({
  read,
  projectId = null,
  busy = false,
  enabled = true,
}: {
  read: WorkspaceReader;
  projectId?: string | null;
  busy?: boolean;
  enabled?: boolean;
}) {
  const latest = useRef(read);
  latest.current = read;
  const generation = useRef(0),
    alive = useRef(false),
    lock = useRef(false);
  const [page, setPage] = useState<WorkspaceHistoryPage | null>(null);
  const [detail, setDetail] = useState<WorkspaceHistoryDetail | null>(null);
  const [back, setBack] = useState<number[]>([]);
  const [loading, setLoading] = useState(false),
    [stale, setStale] = useState(true),
    [message, setMessage] = useState("");
  async function load(
    offset: number,
    revision: string | null,
    previous: number[],
  ) {
    if (lock.current || !enabled) return;
    lock.current = true;
    const token = ++generation.current;
    setLoading(true);
    try {
      const reply = await latest.current({
        type: "history",
        projectId,
        offset,
        revision,
      });
      if (!alive.current || generation.current !== token) return;
      if (
        reply.type !== "history" ||
        reply.value.projectId !== projectId ||
        reply.value.offset !== offset ||
        (revision && reply.value.revision !== revision)
      )
        throw Error("历史页身份或版本不符，请刷新首页。");
      setPage(reply.value);
      setBack(previous);
      setDetail(null);
      setStale(false);
      setMessage("");
    } catch (error) {
      if (alive.current && generation.current === token) {
        setStale(true);
        setMessage(`${String(error)}；保留上次内容，请刷新首页。`);
      }
    } finally {
      if (alive.current && generation.current === token) {
        lock.current = false;
        setLoading(false);
      }
    }
  }
  useEffect(() => {
    alive.current = true;
    lock.current = false;
    setPage(null);
    setDetail(null);
    setBack([]);
    setStale(true);
    setLoading(false);
    if (enabled) void load(0, null, []);
    return () => {
      alive.current = false;
      generation.current++;
      lock.current = false;
    };
  }, [projectId, enabled]);
  async function inspect(sequence: number) {
    if (!page || lock.current || stale || !enabled) return;
    lock.current = true;
    const token = ++generation.current;
    setLoading(true);
    try {
      const reply = await latest.current({
        type: "historyEntry",
        projectId,
        sequence,
        revision: page.revision,
      });
      if (!alive.current || generation.current !== token) return;
      if (
        reply.type !== "historyEntry" ||
        reply.value.projectId !== projectId ||
        reply.value.sequence !== sequence ||
        reply.value.revision !== page.revision
      )
        throw Error("原历史详情身份或版本不符");
      setDetail(reply.value);
      setMessage("");
    } catch (error) {
      if (alive.current && generation.current === token) {
        setStale(true);
        setMessage(`${String(error)}；保留原内容，请刷新首页。`);
      }
    } finally {
      if (alive.current && generation.current === token) {
        lock.current = false;
        setLoading(false);
      }
    }
  }
  const blocked = busy || loading || !enabled;
  return (
    <section className="panel" aria-label="独立历史记录">
      <HelpHeading as="h2" topic="WorkspaceHistory">{projectId ? `项目 ${projectId} 的执行账本` : "本地操作记录"}</HelpHeading>
      <button disabled={blocked} onClick={() => void load(0, null, [])}>
        刷新历史首页
      </button>
      {message && <p role="status">{message}</p>}
      {!page ? (
        <p>历史尚未读取。</p>
      ) : page.total === 0 ? (
        <p>暂无已提交事件</p>
      ) : (
        page.entries.map((entry) => (
          <article key={entry.sequence}>
            <p>
              #{entry.sequence} · {entry.kind} · {entry.at} {entry.principal}
            </p>
            <button
              disabled={blocked || stale}
              onClick={() => void inspect(entry.sequence)}
            >
              查看历史 #{entry.sequence} 原详情
            </button>
          </article>
        ))
      )}
      {page && (
        <nav aria-label="历史分页">
          <button
            disabled={blocked || stale || !back.length}
            onClick={() =>
              void load(back.at(-1)!, page.revision, back.slice(0, -1))
            }
          >
            上一页
          </button>
          <span>
            当前显示 {page.entries.length} / {page.total}
          </span>
          <button
            disabled={blocked || stale || page.nextOffset === null}
            onClick={() =>
              void load(page.nextOffset!, page.revision, [...back, page.offset])
            }
          >
            下一页
          </button>
        </nav>
      )}
      {detail && (
        <section aria-label="原历史详情">
          <pre>{detail.originalJson}</pre>
          <button onClick={() => setDetail(null)}>关闭历史详情</button>
        </section>
      )}
    </section>
  );
}
