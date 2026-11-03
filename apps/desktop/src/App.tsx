import { ErrorText } from "./ErrorText";
import { ExitWorkbench } from "./ExitWorkbench";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Command, Snapshot, HistoricalResult, CommandResult, WorkspaceOverview, WorkspaceQuery, WorkspaceResult } from "./contracts";
import { submitAndRead } from "./commandSubmission";
import { WorkspaceBrowser } from "./WorkspaceBrowser";
import { WorkspaceHistory } from "./WorkspaceHistory";
import { ManagementCountPanel } from "./ManagementCountPanel";
import { ApiCountPanel } from "./ApiCountPanel";
import { ApiGenerationPanel } from "./ApiGenerationPanel";
import { HistoricalCurrencyPanel } from "./HistoricalCurrencyPanel";
import { ProjectRegistration, WorkflowPlans, VerificationJobs } from "./WorkflowPlans";
import { ValidatorCatalog } from "./ValidatorCatalog";
import { IntegrationHub, type IntegrationRole } from "./IntegrationHub";
import { CodexJobs } from "./CodexJobs";
import { OmpJobs } from "./OmpJobs";
import { ApiJobs } from "./ApiJobs";
import { OmpStopDiagnostics } from "./OmpStopDiagnostics";
import { CoreDiagnostics } from "./CoreDiagnostics";
import { OmpFileRestore } from "./OmpFileRestore";
import { CodexStartups } from "./CodexStartups";
import { ProjectPlanning } from "./ProjectPlanning";
import { ProjectRuns } from "./ProjectRuns";
import { BlindReviewPanel } from "./BlindReviewPanel";
import { ManagementReview } from "./ManagementReview";
import { DesktopTasks } from "./DesktopTasks";
import { Maintenance } from "./Maintenance";
import { TeamTimingPanel } from "./TeamTimingPanel";
import { BaselinePanel } from "./BaselinePanel";
import { ExperimentWorkbenches } from "./ExperimentWorkbenches";
import packageInfo from "../../../package.json";

const pages = [
  "工作总览",
  "需求与计划",
  "接入管理",
  "交接记录",
  "项目设置",
  "维护与升级",
] as const;
type Page = (typeof pages)[number];
const icons = ["◈", "▤", "⌘", "≋", "⚙", "↻"];
const desktop = "__TAURI_INTERNALS__" in window;
const time = (s: string) =>
  new Date(s).toLocaleString("zh-CN", { hour12: false });

export function App() {
  const [page, setPage] = useState<Page>("工作总览");
  const [roleFocus, setRoleFocus] = useState<IntegrationRole | null>(null);
  useEffect(() => {
    if(page!=="需求与计划"||!roleFocus)return;
    const frame=requestAnimationFrame(()=>{
      const field=document.getElementById(`planning-${roleFocus==="worker"?"adapter":roleFocus}`);
      field?.closest(".role-model-fields")?.scrollIntoView({block:"start",behavior:"smooth"});
      field?.focus({preventScroll:true});
      setRoleFocus(null);
    });
    return ()=>cancelAnimationFrame(frame);
  },[page,roleFocus]);
  const [data, setData] = useState<Snapshot | null>(null);
  const [busy, setBusy] = useState(false);
  const [snapshotLoading, setSnapshotLoading] = useState(false);
  const snapshotGeneration = useRef(0);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [workspace, setWorkspace] = useState<WorkspaceOverview | null>(null);
  const [ompManagement, setOmpManagement] = useState<{operationId:string;token:number}|null>(null);
  const readWorkspace = (query: WorkspaceQuery) => invoke<WorkspaceResult>("workspace_read", { query });
  async function query(command: Command): Promise<Snapshot | null> {
    snapshotGeneration.current++;
    setSnapshotLoading(false);
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const next = await invoke<Snapshot>("core_request", { command });
      setData(next);
      return next;
    } catch (e) {
      setError(String(e));
      return null;
    } finally {
      setBusy(false);
    }
  }
  async function send(command: Command) {
    snapshotGeneration.current++;
    setSnapshotLoading(false);
    setBusy(true); setError(""); setNotice("");
    try {
      const outcome=await submitAndRead(command,
        c=>invoke<CommandResult>("core_submit",{command:c}),
        ()=>invoke<Snapshot>("core_request",{command:{type:"snapshot"}}));
      if(outcome.type==="unconfirmed"){setError(outcome.error);return false;}
      if(outcome.snapshot) setData(outcome.snapshot);
      if(outcome.type==="accepted" && outcome.refreshError) {
        setNotice("操作已由核心接收；后台任务是否完成请核对原记录。");
        setError(`操作确认已收到，完整视图刷新失败；请重新读取，勿重复提交：${outcome.refreshError}`);
      }
      return true;
    } finally {setBusy(false);}
  }
  async function queryHistorical(command: Command): Promise<HistoricalResult | null> {
    setBusy(true); setError(""); setNotice("");
    try {
      return await invoke<HistoricalResult>("historical_request", { command });
    } catch (e) { setError(String(e)); return null; }
    finally { setBusy(false); }
  }
  async function restartReviewedStops() {
    snapshotGeneration.current++;
    setSnapshotLoading(false);
    setBusy(true);setError("");
    try { const next=await invoke<Snapshot>("restart_reviewed_stops");setData(next);setNotice("核心已重启并重新读取证据。请核对原任务结果，受影响项目保持暂停。");return true; }
    catch(e){setError(String(e));return false;}
    finally{setBusy(false);}
  }
  async function restartDataDiagnostics() {
    snapshotGeneration.current++;
    setSnapshotLoading(false);
    setBusy(true);setError("");
    try {const next=await invoke<Snapshot>("restart_data_diagnostics");setData(next);return true;}
    catch(e){setError(String(e));return false;}
    finally{setBusy(false);}
  }
  const needsFullView = page !== "工作总览" && page !== "维护与升级" && page !== "交接记录";
  useEffect(() => {
    // The landing page uses bounded workspace queries. Load legacy collections
    // only on pages that still need them; leaving a page invalidates late reads.
    const generation = ++snapshotGeneration.current;
    let stopped = false;
    setSnapshotLoading(false);
    if (desktop && needsFullView) {
      setData(null);
      setSnapshotLoading(true);
      setError("");
      void invoke<Snapshot>("core_request", { command: { type: "snapshot" } })
        .then(next => { if (!stopped && snapshotGeneration.current === generation) setData(next); })
        .catch(e => { if (!stopped && snapshotGeneration.current === generation) setError(String(e)); })
        .finally(() => { if (!stopped && snapshotGeneration.current === generation) setSnapshotLoading(false); });
    }
    return () => { stopped = true; snapshotGeneration.current++; };
  }, [page, needsFullView]);
  const verifying = data?.teamImport?.state === "running" || (data?.baselines?.some(b => b.busy || b.timing?.clockLive) ?? false) || (data?.teamTimelines?.some(t => t.clockLive) ?? false) || !!data?.desktopAccess?.checking || (data?.apiJobs?.some(job=>["starting","executing","cancel-requested"].includes(job.state))??false) || (data?.providerConnections?.some(s=>s.state==="checking")??false) || (data?.ompJobs?.some(job=>["starting","executing","cancel-requested"].includes(job.state))??false) || (data?.projectRuns?.some(r=>r.state==="active")??false) || (data?.managementHistory.projects.some(project=>project.running>0)??false) || (data?.verificationJobs.some(job => job.state === "running" || job.state === "cancelling") ?? false) || data?.codexConnection?.state === "checking" || data?.openaiConnection?.state === "checking" || (data?.codexJobs?.some(job=>["starting","executing","reconciling","cancel-requested","cancelling"].includes(job.state)) ?? false);
  useEffect(() => {
    if (!desktop || !needsFullView || !verifying || busy || snapshotLoading || !!data?.ompStopIssues?.length || !!data?.coreDiagnostic) return;
    let stopped = false;
    let timer: number;
    async function poll() {
      try {
        const next = await invoke<Snapshot>("core_request", {command:{type:"snapshot"}});
        if (!stopped) setData(next);
      } catch(e) {if(!stopped)setError(String(e));}
      finally {if(!stopped)timer=window.setTimeout(poll,1000);}
    }
    timer=window.setTimeout(poll,1000);
    return () => {stopped=true;window.clearTimeout(timer);};
  },[needsFullView,verifying,busy,snapshotLoading,data?.ompStopIssues?.length,data?.coreDiagnostic]);
  const planCount = workspace?.legacyPlanCount ?? null;
  const pendingCount = workspace?.pendingLegacyPlanCount ?? null;
  return (
    <div className="shell">
      <aside className="sidebar">
        <a
          className="brand"
          href="#"
          onClick={(e) => {
            e.preventDefault();
            setPage("工作总览");
          }}
        >
          <span className="brand-symbol">
            C<span>／</span>
          </span>
          <span>
            Cerulean
            <br />
            <b>Longing</b>
          </span>
        </a>
        <div className="workspace">
          <span className="workspace-icon">CL</span>
          <span>
            本地工作空间<small>个人 · Windows</small>
          </span>
          <span className="local-dot" />
        </div>
        <ExperimentWorkbenches desktop={desktop} />
        <div className="nav-label">工作台</div>
        <nav>
          {pages.map((p, i) => (
            <button
              key={p}
              aria-current={page === p ? "page" : undefined}
              className={page === p ? "selected" : ""}
              onClick={() => setPage(p)}
            >
              <span>{icons[i]}</span>
              {p}
              {p === "需求与计划" && (pendingCount ?? 0) > 0 && (
                <em>{pendingCount}</em>
              )}
            </button>
          ))}
        </nav>
        <div className="sidebar-footer">
          <div>
            <span className={data || workspace ? "local-dot" : "muted-dot"} />
            {data || workspace ? "本地核心已连接" : desktop ? "正在连接核心" : "浏览器预览"}
          </div>
          <small>工作台版本 · {packageInfo.version}</small>
          <ExitWorkbench enabled={desktop}/>
        </div>
      </aside>
      <main>
        <header>
          <div className="breadcrumb">
            CeruleanLonging <span>/</span> {page}
          </div>
          <span className="pill">本地优先</span>
        </header>
        <div className="content">
          {page === "维护与升级" ? <Maintenance desktop={desktop}/> : data?.coreDiagnostic ? <CoreDiagnostics data={data} busy={busy} error={error} refresh={()=>send({type:"snapshot"})} restart={restartDataDiagnostics} send={send}/> : <>
          {data?.apiRecovery&&<section className="panel" aria-label="API 写入恢复"><h2>API 记录需要重新校验</h2><p>{data.apiRecovery.message}</p><p>暂停所有项目调度并等待后台结束后，可重启核心读取原证据。不会重新发送原模型请求；已有修改保留。</p><button className="primary" disabled={busy||!data.apiRecovery.canRestart} onClick={()=>void restartDataDiagnostics()}>重新校验 API 记录</button></section>}
          <p className="secondary">已复核的管理残片可在工作总览的“查看全局管理复核记录”中按页核对；未决请求和关联启动仍须在原入口处理。</p>
          {!!data?.startupReservations?.length&&<section className="panel" aria-label="启动复核记录"><h2>已复核的半写启动意图</h2><p>原文件保留，以下尝试编号永久禁止复用。复核不代表任务完成或取消。</p>{data.startupReservations.map(r=><details key={r.executor+":"+r.projectId+":"+r.attemptId}><summary>{r.executor} · {r.projectId} · {r.attemptId}</summary><p>{r.reason}</p><p>{time(r.at)}</p></details>)}</section>}
          {!!data?.startupClosureReviews?.length&&<section className="panel" aria-label="关闭复核记录"><h2>已复核的半写关闭记录</h2><p>原文和有效启动身份保留。请在“需求与计划”中核对原启动记录，仍未关闭的项目须重新审阅关闭操作。</p>{data.startupClosureReviews.map(r=><details key={r.relativePath}><summary>{r.executor} · {r.projectId} · {r.attemptId}</summary><p>{r.reason}</p><p style={{overflowWrap:"anywhere"}}>{r.relativePath}</p><p>{time(r.at)}</p></details>)}</section>}
          <OmpStopDiagnostics data={data} send={send} busy={busy} restart={restartReviewedStops}/>
          <div className="page-heading">
            <div>
              <div className="eyebrow">YOUR LOCAL AI WORKSPACE</div>
              <h1>{page === "工作总览" ? "让每一步协作，有迹可循。" : page}</h1>
              <p>
                {page === "工作总览"
                  ? "从一份清晰的计划开始，逐步建立你的模型团队。"
                  : "计划、批准与记录，由同一个本地核心管理。"}
              </p>
            </div>
            <button
              className="primary"
              disabled={busy || !workspace}
              onClick={() => setPage("需求与计划")}
            >
              ＋ 新建计划
            </button>
          </div>
          {!desktop && (
            <div className="banner">
              当前为界面预览。请启动桌面应用以保存计划、批准与本地记录。
            </div>
          )}
          {error && (
            <div role="alert" className="banner error">
              <ErrorText error={error}/>
              <button
                disabled={busy}
                onClick={() => void send({ type: "snapshot" })}
              >
                重新读取记录
              </button>
            </div>
          )}
          {notice && (
            <div role="status" className="banner">
              {notice}
            </div>
          )}
          {(page === "工作总览" || page === "需求与计划") && <WorkspaceBrowser read={readWorkspace} send={send} enabled={desktop} busy={busy} showPlans={page === "需求与计划"} onOverview={setWorkspace}/>}
          {needsFullView && !data && <div role="status" className="banner">{snapshotLoading ? "正在读取此页的完整记录；独立项目与计划入口仍可使用。" : "此页完整记录尚不可用，不能据此判断没有任务或事件。请重新读取；独立项目与计划入口仍可使用。"}<button disabled={busy || snapshotLoading || !desktop} onClick={() => void query({type:"snapshot"})}>读取完整记录</button></div>}
          {page === "工作总览" && (
            <>
              <section className="stats">
                <div>
                  <span>全部计划</span>
                  <strong>{planCount === null ? "—" : planCount.toString().padStart(2, "0")}</strong>
                  <small>保存在本地事件账本</small>
                </div>
                <div>
                  <span>等待确认</span>
                  <strong>
                    {pendingCount === null ? "—" : pendingCount.toString().padStart(2, "0")}
                    <i>待审阅</i>
                  </strong>
                  <small>批准绑定当前计划内容</small>
                </div>
                <div>
                  <span>模型与执行器</span>
                  <strong>
                    <button className="text-button" onClick={() => setPage("接入管理")}>查看接入</button>
                  </strong>
                  <small>连接状态与可用能力请在接入管理中核对</small>
                </div>
              </section>
              <section className="hero">
                <div>
                  <span className="section-tag">从计划到交付</span>
                  <h2>
                    你决定方向，
                    <br />
                    工作台记录每一步。
                  </h2>
                  <p>
                    主指挥制定方案，用户确认后交由副指挥分派。
                    <br />
                    配置模型与执行者，审阅计划后批准执行，并核对产物与验收结果。
                  </p>
                  <button
                    className="primary"
                    disabled={!workspace || busy}
                    onClick={() => setPage("需求与计划")}
                  >
                    创建第一份计划 <span>↗</span>
                  </button>
                </div>
                <div className="team-map">
                  <div className="node user-node">
                    你<span>需求 · 确认</span>
                  </div>
                  <div className="connector" />
                  <div className="node chief-node">
                    主指挥<span>OpenAI · 方案与收口</span>
                  </div>
                  <div className="connector" />
                  <div className="node deputy-node">
                    副指挥<span>OpenAI · 调度与监督</span>
                  </div>
                  <div className="branches">
                    <span>编程 Agent</span>
                    <span>桌面 Agent</span>
                    <span>API 模型</span>
                  </div>
                </div>
              </section>
              <section className="panel">
                <div className="section-heading">
                  <h2>最近记录</h2>
                  <button
                    className="text-button"
                    onClick={() => setPage("交接记录")}
                  >
                    查看全部 →
                  </button>
                </div>
                {!data ? <div className="empty">尚未读取操作记录。请打开“交接记录”查看。</div> : !data.events.length ? (
                  <div className="empty">
                    <span>≋</span>
                    <h3>一切从第一份计划开始</h3>
                    <p>保存与批准后，操作记录会出现在这里。</p>
                  </div>
                ) : (
                  data.events
                    .slice(-4)
                    .reverse()
                    .map((e) => (
                      <div className="event" key={e.sequence}>
                        <span className="event-dot" />
                        <div>
                          {e.summary}
                          <small>{time(e.at)}</small>
                        </div>
                        <span className="event-seq">#{e.sequence}</span>
                      </div>
                    ))
                )}
              </section>
            </>
          )}
          {page === "需求与计划" && data && (
            <div className="plan-layout">
              <TeamTimingPanel data={data} busy={busy} send={send} desktop={desktop}/>
              <BaselinePanel data={data} busy={busy} send={send}/>
              <ProjectPlanning data={data} busy={busy} send={send}/>
              <WorkflowPlans workflows={data?.workflows ?? []} revisions={data?.projectRevisions ?? []} send={send} busy={busy} />
              <HistoricalCurrencyPanel snapshot={data} query={queryHistorical} busy={busy}/>
              <ManagementCountPanel busy={busy} query={command => invoke("count_resolution_request", {command})}/>
              <ApiCountPanel busy={busy} query={command => invoke("count_resolution_request", {command})}/>
              <ApiGenerationPanel busy={busy} query={command => invoke("count_resolution_request", {command})}/>
              <ProjectRuns data={data} send={send} busy={busy}/>
              <BlindReviewPanel data={data} send={send} busy={busy}/>
              <ManagementReview focusOperation={ompManagement} data={data} send={send} query={query} busy={busy}/>
              <CodexStartups data={data} send={send} query={query} busy={busy}/>
              <OmpJobs data={data} send={send} busy={busy} openManagement={operationId=>setOmpManagement({operationId,token:Date.now()})}/>
              <ApiJobs data={data} send={send} busy={busy} openManagement={operationId=>setOmpManagement({operationId,token:Date.now()})}/>
              <DesktopTasks data={data} send={send} busy={busy}/>
              <OmpFileRestore data={data} query={query} busy={busy}/>
              <CodexJobs jobs={data?.codexJobs ?? []} workflows={data?.workflows ?? []} unsentAssignments={(data?.unsentAssignments ?? []).filter(u=>!data?.ompJobs.some(j=>j.projectId===u.projectId&&j.attemptId===u.attemptId))} cancellations={data?.codexCancellations ?? []} send={send} busy={busy} />
              <VerificationJobs jobs={data?.verificationJobs ?? []} detail={data?.verificationDetail ?? null} send={send} busy={busy} />
            </div>
          )}
          {page === "接入管理" && data && (
            <IntegrationHub data={data} busy={busy} send={send} configure={role=>{setRoleFocus(role);setPage("需求与计划");}}/>
          )}
          {page === "交接记录" && <WorkspaceHistory read={readWorkspace} enabled={desktop} busy={busy}/>}
          {page === "项目设置" && (
            <>
            {data && <ProjectRegistration send={send} busy={busy || snapshotLoading} connected={true} />}
            {data && <ValidatorCatalog items={data.validators} send={send} busy={busy || snapshotLoading} connected={true} />}
            <section className="panel settings">
              <h2>工作台信息</h2>
              <dl>
                <dt>产品名称</dt>
                <dd>CeruleanLonging</dd>
                <dt>当前版本</dt>
                <dd>请在“维护与升级”查看实际运行版本</dd>
                <dt>运行方式</dt>
                <dd>Tauri + TypeScript + 独立 Rust 核心进程</dd>
                <dt>本地数据目录</dt>
                <dd>{workspace?.dataDirectory ?? data?.dataDirectory ?? "桌面启动后显示"}</dd>
                <dt>任务执行</dt>
                <dd>通过批准的项目运行与独立验收记录核对</dd>
                <dt>主副指挥</dt>
                <dd>主指挥可选 OpenAI API 或当前 Codex 会话；副指挥可选 OpenAI API 或 DeepSeek。实际来源以项目配置为准。</dd>
              </dl>
            </section>
            </>
          )}
          <footer>
            由你掌控的本地协作空间 <span>CeruleanLonging / FOUNDATION</span>
          </footer>
          </>}
        </div>
      </main>

    </div>
  );
}
