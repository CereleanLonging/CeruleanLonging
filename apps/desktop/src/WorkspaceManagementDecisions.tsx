import { supervisionDeliverySummary } from "./SupervisionFields";
import { useEffect, useRef, useState } from "react";
import type { ManagementSummary, ManagementView, WorkspaceManagementDecisionDetail, WorkspaceManagementDecisionPage, WorkspaceManagementGatewayEvidenceDetail, WorkspaceGatewayEvidence } from "./contracts";
import type { WorkspaceReader } from "./WorkspaceBrowser";
import "./ManagementReview.css";

type Props = { projectId: string; read: WorkspaceReader; busy: boolean };

const states: Record<string, string> = {
  running: "指挥请求进行中",
  unknown: "请求结果待核对",
  ready: "回复待处理",
  applying: "回复正在入账",
  applied: "回复已处理",
  discarded: "已放弃本地请求",
  abandoned: "应用事务已关闭",
  startupClosed: "启动记录已关闭",
};
const purposes: Record<string, string> = { plan: "主指挥规划", refine: "主指挥文档提炼", assign: "副指挥分配", advise: "副指挥建议", accept: "主指挥收口" };
const pageBytes = 64 * 1024;
const detailBytes = 512 * 1024;

const gatewayBytes = 16 * 1024;
const gatewayStates: Record<WorkspaceGatewayEvidence["status"], string> = {
  linked: "旁路与本地原结果关联已校验",
  headerOnly: "已观察到响应头，尚无完整结果关联",
  missing: "未保存网关旁路记录",
  noHeadersObserved: "原结果关联记录表明没有观察到响应头",
  unavailable: "原旁路记录声明留证不可用",
};
const gatewayRejections: Record<string, string> = {
  invalidLength: "长度不符合要求",
  invalidCharacters: "包含不允许的字符",
  repeatedHeader: "响应头重复",
  credentialReflection: "疑似凭证反射",
};

function fields(value: unknown, names: string[]): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    && Object.keys(value).length === names.length
    && names.every(name => Object.prototype.hasOwnProperty.call(value, name));
}

function safeBodyId(value: unknown): value is string {
  return typeof value === "string" && value.length >= 1 && value.length <= 200
    && !/[^A-Za-z0-9_.:/-]/.test(value);
}

function safeGatewayId(value: unknown): value is string {
  return typeof value === "string" && value.length >= 1 && value.length <= 200
    && !/[^A-Za-z0-9_.:-]/.test(value) && !/^(sk-|bearer)/i.test(value);
}

function validGatewayObservation(value: unknown, operationId: string): value is WorkspaceGatewayEvidence {
  if (!fields(value, ["provider", "localId", "bodyResponseId", "status", "headers"])
    || value.provider !== "a6api" || value.localId !== operationId
    || !(value.bodyResponseId === null || safeBodyId(value.bodyResponseId))
    || typeof value.status !== "string" || !Object.prototype.hasOwnProperty.call(gatewayStates, value.status)) return false;
  const hasHeaders = value.status === "linked" || value.status === "headerOnly";
  if (!hasHeaders) return value.headers === null;
  if (!fields(value.headers, ["status", "requestId"])
    || typeof value.headers.status !== "number" || !Number.isInteger(value.headers.status)
    || value.headers.status < 100 || value.headers.status > 599) return false;
  const requestId = value.headers.requestId;
  if (fields(requestId, ["kind"])) return requestId.kind === "missing";
  if (fields(requestId, ["kind", "value"])) return requestId.kind === "accepted" && safeGatewayId(requestId.value);
  return fields(requestId, ["kind", "reason"]) && requestId.kind === "rejected"
    && typeof requestId.reason === "string" && Object.prototype.hasOwnProperty.call(gatewayRejections, requestId.reason);
}

function validGatewayDetail(value: unknown, detail: WorkspaceManagementDecisionDetail): value is WorkspaceManagementGatewayEvidenceDetail {
  return fields(value, ["projectId", "operationId", "revision", "evidenceDigest", "observation"])
    && value.projectId === detail.projectId && value.operationId === detail.record.operationId
    && value.revision === detail.revision && value.evidenceDigest === detail.record.evidenceDigest
    && (value.observation === null || validGatewayObservation(value.observation, detail.record.operationId));
}

function GatewayEvidence({ detail, read, busy, isCurrent }: {
  detail: WorkspaceManagementDecisionDetail; read: WorkspaceReader; busy: boolean; isCurrent: () => boolean;
}) {
  const latest = useRef({ detail, read, busy, isCurrent }); latest.current = { detail, read, busy, isCurrent };
  const alive = useRef(false);
  const lock = useRef(false);
  const generation = useRef(0);
  const [result, setResult] = useState<WorkspaceManagementGatewayEvidenceDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    alive.current = true;
    return () => { alive.current = false; generation.current++; lock.current = false; };
  }, []);

  async function inspectGateway() {
    const request = latest.current;
    if (!alive.current || lock.current || request.busy || !request.isCurrent()) return;
    lock.current = true;
    const token = ++generation.current;
    const current = () => alive.current && generation.current === token && request.isCurrent();
    setLoading(true);
    setFailed(false);
    setResult(null);
    try {
      const reply = await request.read({ type: "managementGatewayEvidence", projectId: request.detail.projectId, operationId: request.detail.record.operationId, revision: request.detail.revision });
      if (!current()) return;
      if (new TextEncoder().encode(JSON.stringify(reply)).length > gatewayBytes
        || !fields(reply, ["type", "value"]) || reply.type !== "managementGatewayEvidence"
        || !validGatewayDetail(reply.value, request.detail)) throw Error("Invalid gateway evidence response");
      setResult(reply.value);
    } catch {
      if (current()) setFailed(true);
    } finally {
      if (current()) { lock.current = false; setLoading(false); }
    }
  }

  const observation = result?.observation;
  const requestId = observation?.headers?.requestId;
  const gatewayId = requestId?.kind === "accepted" ? requestId.value : null;
  return <section className="management-saved-text" aria-label="管理网关留证" aria-busy={loading}>
    <h4>管理网关留证</h4>
    <button disabled={busy || loading} onClick={() => void inspectGateway()}>查看网关留证</button>
    {loading && <p role="status">正在读取本地网关留证…</p>}
    {failed && <p role="alert">网关留证暂不可读取，请核对当前核心版本及本地证据。原管理详情仍保留；本次失败不表示没有留证。</p>}
    {result && <>
      {observation === null ? <p>该管理请求没有 A6API 网关留证路线。</p> : observation && <>
        <p data-gateway-state={observation.status}>{gatewayStates[observation.status]}</p>
        {observation.headers && <p>HTTP 响应状态 {observation.headers.status}</p>}
        {requestId?.kind === "accepted" && <p>响应头请求 ID 已通过本地格式检查。</p>}
        {requestId?.kind === "missing" && <p>响应头中的请求 ID 缺失。</p>}
        {requestId?.kind === "rejected" && <p>响应头请求 ID 已拒绝：{gatewayRejections[requestId.reason]}。未显示原值。</p>}
        {!observation.headers && <p>本次观察没有可显示的响应头请求 ID。</p>}
      </>}
      <table aria-label="网关留证标识" style={{ width: "100%", tableLayout: "fixed", overflowWrap: "anywhere" }}>
        <thead><tr><th scope="col">本地管理操作 ID</th><th scope="col">响应正文 ID</th><th scope="col">网关请求 ID</th></tr></thead>
        <tbody><tr><td><code>{result.operationId}</code></td><td><code>{observation === null ? "不适用" : observation?.bodyResponseId ?? "未保存"}</code></td><td><code>{observation === null ? "不适用" : gatewayId ?? "未取得有效 ID"}</code></td></tr></tbody>
      </table>
      <p className="secondary">旁路为本次读取观察，不包含在原管理记录版本内。同一版本下稍后主动读取可能取得不同的旁路状态。</p>
      <p className="secondary">本地证据关联不等于供应商账单核销。三种 ID 分别保留；响应头 ID 和 HTTP 状态不证明任务成功、费用核销或免费。</p>
    </>}
  </section>;
}


function validSummary(value: ManagementSummary, projectId: string) {
  return value.projectId === projectId
    && !!value.operationId
    && !!value.purpose
    && !!value.state
    && !!value.evidenceDigest
    && (value.message === null || typeof value.message === "string")
    && typeof value.canDiscard === "boolean"
    && typeof value.canReply === "boolean"
    && typeof value.hasWait === "boolean"
    && typeof value.hasReply === "boolean"
    && typeof value.hasDiscard === "boolean";
}

function sameSummary(value: ManagementView, summary: ManagementSummary, projectId: string) {
  return value.projectId === projectId
    && value.operationId === summary.operationId
    && value.purpose === summary.purpose
    && value.state === summary.state
    && value.message === summary.message
    && value.evidenceDigest === summary.evidenceDigest
    && value.canDiscard === summary.canDiscard
    && value.canReply === summary.canReply
    && (value.waitReason !== null) === summary.hasWait
    && (value.userReply !== null) === summary.hasReply
    && (value.discardReason !== null) === summary.hasDiscard;
}

function validPage(value: WorkspaceManagementDecisionPage, projectId: string, offset: number) {
  const end = offset + value.entries.length;
  return value.projectId === projectId
    && !!value.revision
    && value.offset === offset
    && Number.isSafeInteger(offset)
    && offset >= 0
    && Number.isSafeInteger(value.total)
    && value.total >= offset
    && value.entries.length <= 20
    && end <= value.total
    && (value.total === 0 || value.entries.length > 0)
    && (end === value.total ? value.nextOffset === null : value.nextOffset === end)
    && value.entries.every(entry => validSummary(entry, projectId))
    && new Set(value.entries.map(entry => entry.operationId)).size === value.entries.length;
}

function FullText({ decision }: { decision: ManagementView }) {
  return <>
    {decision.message !== null && <div className="management-saved-text"><h4>原请求说明</h4><p className="management-text">{decision.message}</p></div>}
    {decision.supervisionAdvice && <section className="management-saved-text" aria-label="副指挥监督建议">
      <h4>副指挥监督建议</h4>
      <p>任务 {decision.supervisionAdvice.taskId} · 尝试 {decision.supervisionAdvice.attemptId}</p>
      {decision.supervisionAdvice.omp && <p>OMP 原请求 {decision.supervisionAdvice.omp.requestId}</p>}
      {decision.supervisionAdvice.api && <p>API 请求 {decision.supervisionAdvice.api.requestId} · 第 {decision.supervisionAdvice.api.round} 轮</p>}
      {decision.supervisionAdvice.deliveryState === "recordOnly" && !decision.supervisionAdvice.omp && <p>仅保存供审阅，尚未自动投送；不代表执行器已经纠偏。</p>}
      <p>{decision.supervisionAdvice.current ? "原观察仍对应当前运行" : "原观察已过期或执行已结束，仅供审阅"} · {supervisionDeliverySummary(decision.supervisionAdvice)}</p>
      {decision.supervisionAdvice.suggestion ? <><p className="management-text">{decision.supervisionAdvice.suggestion}</p><h4>建议依据</h4><p className="management-text">{decision.supervisionAdvice.rationale}</p></> : <p>尚无可确认的建议正文；请求结果未知时不会自动重发。</p>}
      <details><summary>关联异常</summary><p><code>{decision.supervisionAdvice.incidentId}</code></p><p><code>{decision.supervisionAdvice.observationDigest}</code></p>{decision.supervisionAdvice.omp && <p>原文件工具记录 <code>{decision.supervisionAdvice.omp.toolHead}</code></p>}</details>
    </section>}
    {decision.waitReason !== null && <div className="management-saved-text"><h4>指挥等待原因（原文）</h4><p className="management-text">{decision.waitReason}</p></div>}
    {decision.userReply !== null && <div className="management-saved-text"><h4>已保存的用户答复</h4><p className="management-text">{decision.userReply}</p><p className="secondary">答复供后续指挥请求参考；不改变已批准范围，也不自动恢复调度。</p></div>}
    {decision.discardReason !== null && <div className="management-saved-text"><h4>{["abandoned", "startupClosed"].includes(decision.state) ? "关闭原因" : "放弃本地请求的理由"}</h4><p className="management-text">{decision.discardReason}</p><p className="secondary">{decision.state === "startupClosed" ? "启动记录已在本地关闭，不发送任务、不自动恢复调度，也不退回请求额度；此前可能已经建立远端会话，此处不代表远程取消或退费。" : decision.state === "abandoned" ? "应用事务已在本地关闭，不发送任务、不自动恢复调度，也不退回已记录的请求额度。" : "本地请求已关闭。这不代表供应商已取消处理或退费，系统不会重发此请求。"}</p></div>}
    {!decision.supervisionAdvice && decision.waitReason === null && decision.userReply === null && decision.discardReason === null && <p className="secondary">这条请求没有已保存的等待原因、用户答复或放弃理由。</p>}
  </>;
}

function Evidence({ decision }: { decision: ManagementView }) {
  const targetLabels: Record<string, string> = { propose: "提交方案", assign: "分配任务", accept: "收口任务", advise: "提交建议", wait: "等待补充" };
  return <>
    {decision.applicationRecovery && <section className="management-saved-text" aria-label="未提交的应用事务">
      <h4>未提交的应用事务</h4>
      <dl className="management-identifiers"><dt>应用目标</dt><dd>{targetLabels[decision.applicationRecovery.targetKind] ?? decision.applicationRecovery.targetKind}（{decision.applicationRecovery.targetKind}）</dd><dt>任务</dt><dd>{decision.applicationRecovery.taskId ?? "未关联任务"}</dd><dt>尝试</dt><dd><code>{decision.applicationRecovery.attemptId ?? "未关联尝试"}</code></dd><dt>会话</dt><dd><code>{decision.applicationRecovery.sessionId ?? "未关联会话"}</code></dd><dt>工作流序号</dt><dd>{decision.applicationRecovery.workflowSequence}</dd><dt>证据摘要</dt><dd><code>{decision.applicationRecovery.evidenceDigest}</code></dd></dl>
      <p className="secondary">已保存应用意图；本页只读，不执行应用、发送任务、恢复调度或退回请求额度。</p>
    </section>}
    {decision.adviceTrackingReview && <section className="management-saved-text" aria-label="建议跟踪证据"><h4>建议跟踪证据</h4><p>工作流序号 {decision.adviceTrackingReview.workflowSequence}</p><p>证据摘要 <code>{decision.adviceTrackingReview.evidenceDigest}</code></p><p className="secondary">建议送达结果仍须独立对账，本页不会结束跟踪或重发建议。</p></section>}
    {decision.adviceTrackingClosure && <section className="management-saved-text" aria-label="建议跟踪结束记录"><h4>建议跟踪已由用户结束</h4><p>送达仍未知；原建议和发送证据保留。</p><p className="management-text">{decision.adviceTrackingClosure.reason}</p><p className="secondary">{decision.adviceTrackingClosure.at}</p></section>}
  </>;
}

export function WorkspaceManagementDecisions(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(false);
  const lock = useRef(false);
  const generation = useRef(0);
  const detailGeneration = useRef(0);
  const gatewayGeneration = useRef(0);
  const [gatewayEpoch, setGatewayEpoch] = useState(0);
  const shown = useRef<WorkspaceManagementDecisionPage | null>(null);
  const [page, setPage] = useState<WorkspaceManagementDecisionPage | null>(null);
  const [detail, setDetail] = useState<WorkspaceManagementDecisionDetail | null>(null);
  const [back, setBack] = useState<number[]>([]);
  const [loading, setLoading] = useState(false);
  const [stale, setStale] = useState(false);
  const [error, setError] = useState("");
  const blocked = loading || props.busy;

  function invalidateGateway() {
    gatewayGeneration.current++;
    setGatewayEpoch(gatewayGeneration.current);
  }

  async function run(action: (current: () => boolean) => Promise<void>) {
    if (!alive.current || lock.current || latest.current.busy) return;
    lock.current = true;
    const token = ++generation.current;
    const projectId = latest.current.projectId;
    const current = () => alive.current && generation.current === token && latest.current.projectId === projectId;
    setLoading(true);
    try {
      await action(current);
      if (current()) setError("");
    } catch (value) {
      if (current()) {
        setError(String(value));
        setStale(true);
      }
    } finally {
      if (current()) {
        lock.current = false;
        setLoading(false);
      }
    }
  }

  async function readPage(offset: number, revision: string | null, history: number[], current: () => boolean) {
    invalidateGateway();
    const projectId = latest.current.projectId;
    const reply = await latest.current.read({ type: "managementDecisions", projectId, offset, revision });
    if (!current()) return;
    if (new TextEncoder().encode(JSON.stringify(reply)).length > pageBytes) throw Error("原管理决策页超过有界读取大小，请刷新。");
    if (reply.type !== "managementDecisions" || !validPage(reply.value, projectId, offset) || (revision !== null && (reply.value.revision !== revision || reply.value.total !== shown.current?.total))) throw Error("原管理决策页身份、游标或版本不符，请刷新。");
    const previous = shown.current;
    if (previous?.revision === reply.value.revision && previous.total !== reply.value.total) throw Error("同一管理决策版本的总数不一致，请刷新。");
    if (previous?.revision === reply.value.revision && previous.offset === offset && (previous.nextOffset !== reply.value.nextOffset || JSON.stringify(previous.entries) !== JSON.stringify(reply.value.entries))) throw Error("同一管理决策版本的原内容不一致，请刷新。");
    if (previous?.revision !== reply.value.revision || previous?.offset !== offset) {
      detailGeneration.current++;
      setDetail(null);
    }
    shown.current = reply.value;
    setPage(reply.value);
    setBack(history);
    setStale(false);
  }

  useEffect(() => {
    alive.current = true;
    lock.current = false;
    shown.current = null;
    setPage(null);
    setDetail(null);
    setBack([]);
    setLoading(false);
    setStale(false);
    setError("");
    void run(current => readPage(0, null, [], current));
    return () => {
      alive.current = false;
      generation.current++;
      detailGeneration.current++;
      gatewayGeneration.current++;
      lock.current = false;
    };
  }, [props.projectId]);

  useEffect(() => {
    if (!page && !loading && !stale && !props.busy) void run(current => readPage(0, null, [], current));
  }, [page, loading, stale, props.busy]);

  function inspect(summary: ManagementSummary) {
    if (!page || stale || lock.current || latest.current.busy || !alive.current) return;
    const projectId = latest.current.projectId;
    const revision = page.revision;
    const detailToken = ++detailGeneration.current;
    invalidateGateway();
    void run(async current => {
      const reply = await latest.current.read({ type: "managementDecision", projectId, operationId: summary.operationId, revision });
      if (!current() || detailGeneration.current !== detailToken) return;
      if (new TextEncoder().encode(JSON.stringify(reply)).length > detailBytes) throw Error("原管理决策详情超过有界读取大小，请刷新。");
      if (reply.type !== "managementDecision" || reply.value.projectId !== projectId || reply.value.revision !== revision || !sameSummary(reply.value.record, summary, projectId)) throw Error("原管理决策详情身份、摘要或版本不符，请刷新。");
      setDetail(reply.value);
    });
  }

  return <section className="panel" aria-label="项目管理决策记录" aria-busy={loading}>
    <div className="section-heading"><h3>项目 {props.projectId} 的管理决策</h3><button disabled={blocked} onClick={() => void run(current => readPage(0, null, [], current))}>刷新管理决策首页</button></div>
    <p>这里只读取原主副指挥决策及其完整证据，保留未知结果供核对。本页不发送请求、不批准、不答复、不放弃，也不自动恢复调度。</p>
    {loading && <p role="status">正在读取原管理决策记录…</p>}
    {error && <p role="alert">{error} 上次内容仅供核对，请刷新后继续。</p>}
    {!page ? <p>管理决策记录尚未取得，不能据此判断没有请求。</p> : <>
      <p>原项目共 {page.total} 条管理决策</p>
      {page.total === 0 ? <p>原项目尚无管理决策；可刷新核对最新记录。</p> : page.entries.map(summary => <article className="panel management-entry" key={summary.operationId} data-management-operation={summary.operationId}>
        <div className="section-heading"><h4>{purposes[summary.purpose] ?? summary.purpose}</h4><span className="badge">{states[summary.state] ?? summary.state}</span></div>
        <dl className="management-identifiers"><dt>项目</dt><dd>{summary.projectId}</dd><dt>请求</dt><dd><code>{summary.operationId}</code></dd></dl>
        {summary.message && <p className="management-text">{summary.message}</p>}
        <p className="secondary">{[summary.hasWait && "有等待原因", summary.hasReply && "有用户答复", summary.hasDiscard && "有关闭或放弃理由"].filter(Boolean).join(" · ") || "请求摘要"}。全文请查看完整记录。</p>
        {summary.state === "unknown" && <p className="secondary">目前没有可确认的结果，系统不会自动重发或释放原请求。</p>}
        <button disabled={blocked || stale} onClick={() => inspect(summary)}>查看管理决策 {summary.operationId} 的原详情</button>
      </article>)}
      <nav aria-label="管理决策分页"><button disabled={blocked || stale || !back.length} onClick={() => void run(current => readPage(back.at(-1)!, page.revision, back.slice(0, -1), current))}>上一页</button><span>当前显示 {page.entries.length} / {page.total}</span><button disabled={blocked || stale || page.nextOffset === null} onClick={() => void run(current => readPage(page.nextOffset!, page.revision, [...back, page.offset], current))}>下一页</button></nav>
    </>}
    {detail && <section className="panel management-detail" aria-label="管理决策原详情" data-management-detail={detail.record.operationId}>
      <div className="section-heading"><h4>管理决策原详情</h4><button onClick={() => { detailGeneration.current++; invalidateGateway(); setDetail(null); }}>关闭管理决策详情</button></div>
      <dl className="management-identifiers"><dt>项目</dt><dd>{detail.record.projectId}</dd><dt>请求</dt><dd><code>{detail.record.operationId}</code></dd><dt>目的</dt><dd>{purposes[detail.record.purpose] ?? detail.record.purpose}</dd><dt>状态</dt><dd>{states[detail.record.state] ?? detail.record.state}</dd><dt>证据摘要</dt><dd><code>{detail.record.evidenceDigest}</code></dd><dt>可答复</dt><dd>{detail.record.canReply ? "是" : "否"}</dd><dt>可放弃</dt><dd>{detail.record.canDiscard ? "是" : "否"}</dd></dl>
      <FullText decision={detail.record}/>
      <Evidence decision={detail.record}/>
      <GatewayEvidence key={gatewayEpoch} detail={detail} read={props.read} busy={blocked || stale}
        isCurrent={() => alive.current && gatewayGeneration.current === gatewayEpoch && latest.current.projectId === detail.projectId}/>
      <p className="secondary">此详情按版本 {detail.revision} 读取；页面只展示原始证据，不执行任何处置。</p>
      <details><summary>查看原管理决策全部字段</summary><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{JSON.stringify(detail.record, null, 2)}</pre></details>
    </section>}
  </section>;
}
