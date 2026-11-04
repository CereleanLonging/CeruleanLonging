import { HelpHeading } from "./HelpHeading";
import { useEffect, useRef, useState } from "react";
import type { WorkspaceConfigurationDetail, WorkspaceConfigurationPage, WorkspaceConfigurationSummary } from "./contracts";
import type { WorkspaceReader } from "./WorkspaceBrowser";

type Props = { read: WorkspaceReader; enabled: boolean; busy: boolean };
export function WorkspaceConfigurations(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(true), lock = useRef(false), generation = useRef(0);
  const [page, setPage] = useState<WorkspaceConfigurationPage | null>(null), [back, setBack] = useState<number[]>([]);
  const [detail, setDetail] = useState<WorkspaceConfigurationDetail | null>(null);
  const [loading, setLoading] = useState(false), [stale, setStale] = useState(false), [error, setError] = useState("");
  const blocked = loading || props.busy || !props.enabled;
  async function run(action: (current: () => boolean) => Promise<void>) {
    if (lock.current || !latest.current.enabled) return;
    lock.current = true; const token = ++generation.current;
    const current = () => alive.current && generation.current === token && latest.current.enabled;
    setLoading(true); setError("");
    try { await action(current); }
    catch (e) { if (current()) { setError(String(e)); setStale(true); } }
    finally { if (current()) { lock.current = false; setLoading(false); } }
  }
  async function readPage(offset: number, revision: string | null, history: number[], current: () => boolean) {
    const reply = await latest.current.read({ type: "configurations", offset, revision });
    if (!current()) return;
    if (reply.type !== "configurations") throw Error("原配置清单类型不符，请刷新。");
    const value = reply.value;
    if (value.offset !== offset || !value.revision || (revision !== null && value.revision !== revision) || !Number.isSafeInteger(value.total) || value.total < 0 || (value.nextOffset !== null && (!Number.isSafeInteger(value.nextOffset) || value.nextOffset <= offset)) || value.entries.some(s => !s.projectId || !s.digest) || new Set(value.entries.map(s => s.projectId)).size !== value.entries.length) throw Error("原配置清单身份、游标或版本不符，请刷新。");
    setPage(value); setBack(history); setDetail(null); setStale(false);
  }
  useEffect(() => {
    alive.current = true; lock.current = false; setLoading(false);
    if (props.enabled) void run(current => readPage(0, null, [], current));
    return () => { alive.current = false; generation.current++; lock.current = false; };
  }, [props.enabled]);
  function readDetail(summary: WorkspaceConfigurationSummary) {
    if (!page) return;
    const revision = page.revision, projectId = summary.projectId, digest = summary.digest;
    void run(async current => {
      const reply = await latest.current.read({ type: "configuration", projectId, revision });
      if (!current()) return;
      if (reply.type !== "configuration" || reply.value.projectId !== projectId || reply.value.revision !== revision || reply.value.saved.setup.projectId !== projectId || reply.value.saved.digest !== digest) throw Error("原配置详情项目、版本或摘要不符，请刷新。");
      setDetail(reply.value);
    });
  }
  return <section className="panel" aria-label="已保存项目配置">
    <div className="section-heading"><HelpHeading as="h3" topic="WorkspaceConfigurations">已保存项目配置</HelpHeading><button disabled={blocked} onClick={() => void run(current => readPage(0, null, [], current))}>刷新配置首页</button></div>
    <p>读取本地保存的原执行方案与摘要。查看配置不会启动规划、派发任务或授权执行。</p>
    {error && <p role="alert">{error} 上次内容仅供核对，请刷新后继续。</p>}
    {!page ? <p>配置清单尚未取得。</p> : <>
      <p>共 {page.total} 项已保存配置</p>
      {page.total === 0 ? <p>尚无已保存项目配置。</p> : page.entries.map(summary => <article className="panel" key={summary.projectId}>
        <HelpHeading as="h4" topic="WorkspaceConfigurations">项目 {summary.projectId}</HelpHeading><p style={{overflowWrap:"anywhere"}}>配置摘要：{summary.digest}</p>
        <button disabled={blocked || stale} onClick={() => readDetail(summary)}>查看项目 {summary.projectId} 的原配置</button>
      </article>)}
      <nav aria-label="配置分页"><button disabled={blocked || stale || !back.length} onClick={() => void run(current => readPage(back.at(-1)!, page.revision, back.slice(0, -1), current))}>上一页</button><span>当前显示 {page.entries.length} / {page.total}</span><button disabled={blocked || stale || page.nextOffset === null} onClick={() => void run(current => readPage(page.nextOffset!, page.revision, [...back, page.offset], current))}>下一页</button></nav>
    </>}
    {detail && <section className="panel" aria-label="原项目配置详情">
      <HelpHeading as="h4" topic="WorkspaceConfigurations">项目 {detail.projectId} 的原配置</HelpHeading>
      <p style={{overflowWrap:"anywhere"}}>原配置摘要：{detail.saved.digest}</p>
      <p>配置版本 {detail.saved.setup.version} · 主指挥 {detail.saved.setup.chiefModel} · 副指挥 {detail.saved.setup.deputyModel}</p>
      <p style={{whiteSpace:"pre-wrap",overflowWrap:"anywhere"}}>{detail.saved.setup.requirement}</p>
      <HelpHeading as="h5" topic="WorkspaceConfigurations">完整原保存方案</HelpHeading><pre style={{whiteSpace:"pre-wrap",overflowWrap:"anywhere"}}>{JSON.stringify(detail.saved.setup,null,2)}</pre>
    </section>}
  </section>;
}
