import "./workbench-theme.css";
import { HelpHeading } from "./HelpHeading";
import { AccessStorageStatus } from "./accessStorage";
import { ErrorText } from "./ErrorText";
import { ExitWorkbench } from "./ExitWorkbench";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Command, Snapshot, HistoricalResult, CommandResult, WorkspaceOverview, WorkspaceQuery, WorkspaceResult } from "./contracts";
import { submitAndRead } from "./commandSubmission";
import { WorkspaceBrowser } from "./WorkspaceBrowser";
import { WorkspaceHistory } from "./WorkspaceHistory";
import { WorkflowMap } from "./WorkflowMap";
import { ManagementCountPanel } from "./ManagementCountPanel";
import { ApiCountPanel } from "./ApiCountPanel";
import { ApiGenerationPanel } from "./ApiGenerationPanel";
import { HistoricalCurrencyPanel } from "./HistoricalCurrencyPanel";
import { ProjectRegistration, WorkflowPlans, VerificationJobs } from "./WorkflowPlans";
import { ValidatorCatalog } from "./ValidatorCatalog";
import type { IntegrationRole, ChiefMethod, ConfigurationRequest } from "./IntegrationHub";
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
import { ExperimentLab } from "./ExperimentLab";
import packageInfo from "../../../package.json";

const pages = [
  "工作总览",
  "接入管理",
  "需求与计划",
  "交接记录",
  "项目设置",
  "独立实验工作台",
  "维护与升级",
] as const;
type Page = (typeof pages)[number];
const icons = ["◈", "⌘", "▤", "≋", "⚙", "⚗", "↻"];
const desktop = "__TAURI_INTERNALS__" in window;
const time = (s: string) =>
  new Date(s).toLocaleString("zh-CN", { hour12: false });

export function App() {
  const windowLabel = desktop ? getCurrentWindow().label : "main";
  const experiment = windowLabel.startsWith("experiment-");
  const [page, setPage] = useState<Page>("工作总览");
  useEffect(() => { window.scrollTo({ top: 0, behavior: "instant" }); }, [page]);
  const [experimentsEnabled,setExperimentsEnabled]=useState(false);
  const [configuration,setConfiguration]=useState<ConfigurationRequest|null>(null);
  const configurationSequence=useRef(0);
  const [data, setData] = useState<Snapshot | null>(null);
  // Keep the project draft mounted while page-specific snapshots are loading.
  // A cached view is read-only until the current page has a fresh snapshot.
  const planningSnapshot = useRef<Snapshot | null>(null);
  if (data) planningSnapshot.current = data;
  const [busy, setBusy] = useState(false);
  const [snapshotLoading, setSnapshotLoading] = useState(false);
  const snapshotGeneration = useRef(0);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [workspace, setWorkspace] = useState<WorkspaceOverview | null>(null);
  function navigate(next:Page){setConfiguration(null);setPage(next);}
  function configure(role:IntegrationRole,chiefMethod?:ChiefMethod,projectId?:string){
    setConfiguration({token:++configurationSequence.current,role,chiefMethod,projectId});
    setPage("接入管理");
  }
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
  async function send(command: Command, onError?: (message: string) => void) {
    snapshotGeneration.current++;
    setSnapshotLoading(false);
    setBusy(true); setError(""); setNotice("");
    try {
      const outcome=await submitAndRead(command,
        c=>invoke<CommandResult>("core_submit",{command:c}),
        ()=>invoke<Snapshot>("core_request",{command:{type:"snapshot"}}));
      if(outcome.type==="unconfirmed"){setError(outcome.error);onError?.(outcome.error);return false;}
      if(outcome.snapshot) setData(outcome.snapshot);
      if(outcome.type==="accepted" && outcome.refreshError) {
        setNotice("操作已由核心接收；后台任务是否完成请核对原记录。");
        const message=`操作确认已收到，完整视图刷新失败；请重新读取，勿重复提交：${outcome.refreshError}`;
        setError(message);onError?.(message);
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
  const needsFullView = page === "独立实验工作台" ? experimentsEnabled : page !== "工作总览" && page !== "维护与升级" && page !== "交接记录";
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
    if (!desktop || !needsFullView || !(verifying || data?.desktopAccess?.tasks?.some(task=>task.active||task.stopping) || data?.managementDecisions?.some(d=>d.state==="awaitingExternal")) || busy || snapshotLoading || !!data?.ompStopIssues?.length || !!data?.coreDiagnostic) return;
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
  },[needsFullView,verifying,busy,snapshotLoading,data?.ompStopIssues?.length,data?.coreDiagnostic,data?.desktopAccess?.tasks?.some(task=>task.active||task.stopping),data?.managementDecisions?.some(d=>d.state==="awaitingExternal")]);
  const planCount = workspace?.legacyPlanCount ?? null;
  const pendingCount = workspace?.pendingLegacyPlanCount ?? null;
  const showPlanning = (page === "接入管理" || page === "需求与计划") && !data?.coreDiagnostic;
  const standardPage = page !== "维护与升级" && page !== "独立实验工作台" && !data?.coreDiagnostic;
  return (
    <div className="shell" data-workbench={experiment ? "experiment" : "daily"}>
      <aside className="sidebar">
        <a
          className="brand"
          aria-label="CeruleanLonging 工作总览"
          href="#"
          onClick={(e) => {
            e.preventDefault();
            navigate("工作总览");
          }}
        >
          <span className="brand-monogram" aria-hidden="true">C</span>
          <span className="brand-slash" aria-hidden="true" />
          <span className="brand-name" aria-hidden="true">
            <span>Cerulean</span>
            <span>Longing</span>
          </span>
        </a>
        <div className="workspace">
          <span className="workspace-icon">CL</span>
          <span>
            {experiment ? "独立实验台" : "日常工作台"}
            <small>{experiment ? windowLabel.slice("experiment-".length) : "个人 · Windows"}</small>
          </span>
          <span className="local-dot" />
        </div>
        <div className="nav-label">工作台</div>
        <nav>
          {pages.map((p, i) => (
            <button
              key={p}
              aria-current={page === p ? "page" : undefined}
              className={page === p ? "selected" : ""}
              onClick={() => navigate(p)}
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
          <AccessStorageStatus/>
          {page === "维护与升级" ? <Maintenance desktop={desktop}/> : page === "独立实验工作台" ? <ExperimentLab desktop={desktop} enabled={experimentsEnabled} onEnabled={setExperimentsEnabled} data={data} busy={busy} loading={snapshotLoading} error={error} refresh={()=>void query({type:"snapshot"})} send={send}/> : data?.coreDiagnostic ? <CoreDiagnostics data={data} busy={busy} error={error} refresh={()=>send({type:"snapshot"})} restart={restartDataDiagnostics} send={send}/> : <>
          {data?.apiRecovery&&<section className="panel" aria-label="API 写入恢复"><HelpHeading as="h2" topic="App">API 记录需要重新校验</HelpHeading><p>{data.apiRecovery.message}</p><p>暂停所有项目调度并等待后台结束后，可重启核心读取原证据。不会重新发送原模型请求；已有修改保留。</p><button className="primary" disabled={busy||!data.apiRecovery.canRestart} onClick={()=>void restartDataDiagnostics()}>重新校验 API 记录</button></section>}
          {!!data?.startupReservations?.length&&<section className="panel" aria-label="启动复核记录"><HelpHeading as="h2" topic="App">已复核的半写启动意图</HelpHeading><p>原文件保留，以下尝试编号永久禁止复用。复核不代表任务完成或取消。</p>{data.startupReservations.map(r=><details key={r.executor+":"+r.projectId+":"+r.attemptId}><summary>{r.executor} · {r.projectId} · {r.attemptId}</summary><p>{r.reason}</p><p>{time(r.at)}</p></details>)}</section>}
          {!!data?.startupClosureReviews?.length&&<section className="panel" aria-label="关闭复核记录"><HelpHeading as="h2" topic="App">已复核的半写关闭记录</HelpHeading><p>原文和有效启动身份保留。请在“需求与计划”中核对原启动记录，仍未关闭的项目须重新审阅关闭操作。</p>{data.startupClosureReviews.map(r=><details key={r.relativePath}><summary>{r.executor} · {r.projectId} · {r.attemptId}</summary><p>{r.reason}</p><p style={{overflowWrap:"anywhere"}}>{r.relativePath}</p><p>{time(r.at)}</p></details>)}</section>}
          <OmpStopDiagnostics data={data} send={send} busy={busy} restart={restartReviewedStops}/>
          <div className="page-heading">
            <div>
              <HelpHeading as="h1" topic="App">{page === "工作总览" ? "让每一步协作，有迹可循。" : page}</HelpHeading>
              <p>
                {page === "工作总览"
                  ? "从一份清晰的计划开始，逐步建立你的模型团队。"
                  : "计划、批准与记录，由同一个本地核心管理。"}
              </p>
            </div>
            <button
              className="primary"
              disabled={busy || !workspace}
              onClick={() => configure("chief")}
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
          {page === "工作总览" && <WorkspaceBrowser read={readWorkspace} send={send} enabled={desktop} busy={busy} showPlans={false} onOverview={setWorkspace}/>}
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
                    <button className="text-button" onClick={() => navigate("接入管理")}>查看接入</button>
                  </strong>
                  <small>连接状态与可用能力请在接入管理中核对</small>
                </div>
              </section>
              <WorkflowMap />
              <section className="panel">
                <div className="section-heading">
                  <HelpHeading as="h2" topic="App">最近记录</HelpHeading>
                  <button
                    className="text-button"
                    onClick={() => navigate("交接记录")}
                  >
                    查看全部 →
                  </button>
                </div>
                {!data ? <div className="empty">尚未读取操作记录。请打开“交接记录”查看。</div> : !data.events.length ? (
                  <div className="empty">
                    <span>≋</span>
                    <HelpHeading as="h3" topic="App">一切从第一份计划开始</HelpHeading>
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
          </>}
          {planningSnapshot.current && <div hidden={!showPlanning}>
            <ProjectPlanning key={planningSnapshot.current.dataDirectory} data={data ?? planningSnapshot.current}
              busy={busy || snapshotLoading || !data || !showPlanning}
              mode={page === "接入管理" ? "connections" : "planning"} send={send}
              configuration={configuration} onConfigurationReady={token=>setConfiguration(c=>c?.token===token?null:c)}
              onSetup={()=>setPage("项目设置")} onConnections={()=>configure("chief")}
              onContinue={()=>{navigate("需求与计划");window.scrollTo({top:0});}}/>
          </div>}
          {standardPage && <>
          {page === "需求与计划" && <details className="panel" aria-label="手工计划与历史记录">
            <summary>手工计划与历史记录（可选）</summary>
            <p className="secondary">这里单独保存手工任务清单，不使用上方尚未提交的项目接入草稿。编程项目请在上方填写需求，已选角色模型会继续沿用。</p>
            <WorkspaceBrowser read={readWorkspace} send={send} enabled={desktop} busy={busy} showPlans onOverview={setWorkspace}/>
          </details>}
          {page === "需求与计划" && data && (
            <div className="plan-layout">
              <ProjectRuns data={data} send={send} busy={busy} read={readWorkspace} enabled={desktop}/>
              <BlindReviewPanel data={data} send={send} busy={busy}/>
              <details className="panel project-data-statistics" aria-label="项目数据与费用统计">
                <summary>项目数据与费用统计<span className="secondary">执行方案、历史费用及费用核对，按需展开</span></summary>
                <div className="project-statistics-content">
                  <WorkflowPlans workflows={data.workflows ?? []} revisions={data.projectRevisions ?? []} setups={data.projectSetups ?? []} send={send} busy={busy} />
                  <HistoricalCurrencyPanel snapshot={data} query={queryHistorical} busy={busy}/>
                  <ManagementCountPanel busy={busy} query={command => invoke("count_resolution_request", {command})}/>
                  <ApiCountPanel busy={busy} query={command => invoke("count_resolution_request", {command})}/>
                  <ApiGenerationPanel busy={busy} query={command => invoke("count_resolution_request", {command})}/>
                </div>
              </details>
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
          {page === "交接记录" && <WorkspaceHistory read={readWorkspace} enabled={desktop} busy={busy}/>}
          {page === "项目设置" && (
            <>
            {configuration&&<section className="panel" aria-label="模型配置引导"><HelpHeading as="h2" topic="App">继续配置{configuration.role==="chief"?"主指挥":configuration.role==="deputy"?"副指挥":"执行端"}</HelpHeading><p>先在下方关联已有项目目录，再返回接入管理。{configuration.chiefMethod==="desktop"?"已保留“当前 Codex 聊天”的选择；进入后填写会话标识。":"关联目录不会调用模型。"}</p><button className="primary" disabled={busy||snapshotLoading||!data?.workflows.length} onClick={()=>setPage("接入管理")}>继续配置模型</button><p className="secondary">独立检查与盲评验收分别固定关联；主指挥将在方案中提供客观测试，审阅批准后执行。配置、请求方案、批准与启动保持分别确认。</p></section>}
            {data && <ProjectRegistration send={send} busy={busy || snapshotLoading} connected={true} />}
            {data && <ValidatorCatalog items={data.validators} send={send} busy={busy || snapshotLoading} connected={true} />}
            <section className="panel settings">
              <HelpHeading as="h2" topic="App">工作台信息</HelpHeading>
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
