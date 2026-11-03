import { useEffect, useRef, useState } from "react";
import type { Command, WorkspaceExecutionReview } from "./contracts";
import { ExecutionAuthorization } from "./ExecutionAuthorization";
import { WorkerAccountAuthorization } from "./WorkerAccountAuthorization";
import type { WorkspaceReader } from "./WorkspaceBrowser";
type Props = { projectId: string; expectedDigest: string | null; read: WorkspaceReader; send: (command: Command) => Promise<boolean>; busy: boolean; close: () => void };
export function WorkspaceExecution(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(true), lock = useRef(false), generation = useRef(0);
  const [view, setView] = useState<WorkspaceExecutionReview | null>(null), [loading, setLoading] = useState(false), [stale, setStale] = useState(true);
  const [pending, setPending] = useState<{digest:string;accepted:boolean}|null>(null), [message, setMessage] = useState("");
  const [accountScope,setAccountScope] = useState<string|null>(null);
  const scopeKey=view?.workerAccountSetupDigest ? `${view.project.id}:${view.revisionDigest}:${view.workerAccountSetupDigest}` : null;
  const blocked = loading || props.busy;
  async function run(action: (current: () => boolean) => Promise<void>) {
    if (lock.current) return;
    lock.current = true; const token = ++generation.current;
    const current = () => alive.current && generation.current === token;
    setLoading(true); setMessage("");
    try { await action(current); }
    catch (e) { if (current()) { setStale(true); setMessage(String(e)); } }
    finally { if (current()) { lock.current = false; setLoading(false); } }
  }
  async function read(digest: string | null, current: () => boolean) {
    const projectId = latest.current.projectId;
    const result = await latest.current.read({type:"executionReview",projectId,digest});
    if (!current()) return null;
    if (result.type !== "executionReview" || result.value.project.id !== projectId) throw Error("执行授权结果的原项目身份不符。");
    const next = result.value;
    if ((digest !== null && next.revisionDigest !== digest) || (!!next.revision !== !!next.revisionDigest) || (next.revision && next.revision.project.id !== projectId) || (next.approval && next.approval.revisionDigest !== next.revisionDigest)) throw Error("原方案或批准摘要不符，请保留原操作并核对。");
    setView(next); setStale(false); return next;
  }
  useEffect(() => {
    alive.current = true; lock.current = false; setView(null); setPending(null); setAccountScope(null); setStale(true);
    void run(current => read(props.expectedDigest, current).then(()=>{}));
    return () => { alive.current = false; generation.current++; lock.current = false; };
  }, [props.projectId, props.expectedDigest]);
  function checkOriginal() {
    const original = pending;
    void run(async current => {
      const next = await read(original?.digest ?? view?.revisionDigest ?? props.expectedDigest, current);
      if (!next || !current() || !original) return;
      if (next.approval?.revisionDigest === original.digest) { setPending(null); setMessage("已读取原方案的用户批准；本次只回查，没有再次批准。"); }
      else setMessage(original.accepted ? "已收到原操作回执，尚未读到对应批准；继续保留原结果并核对。" : "尚未读到原批准，原操作仍待核对，不会自动重发。");
    });
  }
  function approve() {
    if (!view?.revision || !view.revisionDigest || view.approval || view.revisionRequested || blocked || stale || pending) return;
    const digest = view.revisionDigest, projectId = view.project.id;
    const setupDigest = scopeKey && accountScope===scopeKey ? view.workerAccountSetupDigest : null;
    void run(async current => {
      setPending({digest,accepted:false}); let accepted = false;
      try { accepted = await latest.current.send(setupDigest ? {type:"approveRevisionWithWorkerAccounts",projectId,digest,setupDigest} : {type:"approveRevision",projectId,digest}); } catch { /* Keep original outcome unknown. */ }
      if (!current()) return;
      setPending({digest,accepted});
      if (!accepted) { setMessage("原批准结果尚未确认。仅可读取原记录核对，不会自动再次提交。"); return; }
      // Do not convert a failed read into an unsuccessful approval or resend.
      try {
        const next = await read(digest,current); if (!next || !current()) return;
        if (next.approval?.revisionDigest === digest) { setPending(null); setMessage(next.approval.workerAccountSetup ? "原方案已批准，隔离账户与目录权限授权已记录；请在项目设置核对本次准备结果。任务仍需明确启动。" : "原方案已批准。本次只记录执行授权，后续仍需明确启动。"); }
        else setMessage("已收到原批准回执，原记录尚待核对；请只读回查。");
      } catch(e) { if(current()) { setStale(true); setMessage(`已收到原批准回执，读取失败；不会重复批准：${String(e)}`); } }
    });
  }
  return <div className="modal-backdrop"><section className="modal panel execution-review" role="dialog" aria-modal="true" aria-label="独立执行授权审阅">
    <h2>审阅项目执行方案</h2><p>项目 {props.projectId}</p>
    {message && <p role="status">{message}</p>}
    {!view ? <p>完整授权内容尚未取得，不能确认。</p> : !view.revision ? <p>原项目尚无完整执行方案。</p> : <>
      <ExecutionAuthorization revision={view.revision}/>
      {!view.approval&&<WorkerAccountAuthorization available={!!scopeKey} directories={view.workerAccountDirectories} checked={!!scopeKey&&accountScope===scopeKey} disabled={blocked||stale||!!pending||!!view.revisionRequested} onChange={v=>setAccountScope(v?scopeKey:null)}/>}
      {view.approval?.workerAccountSetup&&<p>已授权本方案的隔离账户创建和项目目录权限设置。只读回查不会重新发起 UAC；准备结果见项目设置。</p>}
      {view.revisionRequested && <p>原方案已请求修订：{view.revisionRequested}。请审阅后续新方案。</p>}
      {view.approval && <p>原方案已由用户批准。批准时间：{view.approval.approvedAt}</p>}
      <p>授权仅对应以上完整原方案；目录、模型、程序、权限或预算变化后须重新审阅。</p>
    </>}
    {pending && <p>原批准待核对，已冻结当前方案；{pending.accepted ? "已收到操作回执。" : "尚未收到可确认回执。"}</p>}
    <div className="card-footer"><button disabled={blocked} onClick={checkOriginal}>{pending ? "读取原批准核对" : "重新读取原方案"}</button><button disabled={blocked} onClick={props.close}>关闭审阅</button><button className="primary" disabled={blocked || stale || !!pending || !view?.revision || !view.revisionDigest || !!view.approval || !!view.revisionRequested} onClick={approve}>确认执行授权</button></div>
  </section></div>;
}
