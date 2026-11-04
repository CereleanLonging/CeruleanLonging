import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { MarkdownImport } from "./MarkdownImport";
import type { RefinementApi } from "./MarkdownRefinement";
import { useEffect, useRef, useState } from "react";
import type { SavedProjectSetup, Command, Plan, PlanPage, ProjectPage, WorkspaceOverview, WorkspaceQuery, WorkspaceResult } from "./contracts";
export type WorkspaceReader = (query: WorkspaceQuery) => Promise<WorkspaceResult>;
type Props = { refinementApi?: RefinementApi; refinementSetups?: SavedProjectSetup[]; configureChief?: (projectId?: string) => void; read: WorkspaceReader; send: (command: Command) => Promise<boolean>; enabled: boolean; busy: boolean; showPlans: boolean; onOverview: (overview: WorkspaceOverview) => void };
type Pending = { type: "create" } | { type: "approve"; id: string; digest: string };

export function WorkspaceBrowser(props: Props) {
  const latest = useRef(props); latest.current = props;
  const alive = useRef(true), lock = useRef(false), generation = useRef(0);
  const [loading, setLoading] = useState(false), [stale, setStale] = useState(true);
  const [overview, setOverview] = useState<WorkspaceOverview | null>(null);
  const [plans, setPlans] = useState<PlanPage | null>(null), [projects, setProjects] = useState<ProjectPage | null>(null);
  const [planBack, setPlanBack] = useState<number[]>([]), [projectBack, setProjectBack] = useState<number[]>([]);
  const [detail, setDetail] = useState<Plan | null>(null), [pending, setPending] = useState<Pending | null>(null);
  const [title, setTitle] = useState(""), [tasks, setTasks] = useState("");
  const [refinementProject,setRefinementProject]=useState("");
  const chief=props.refinementSetups?.find(s=>s.setup.projectId===refinementProject)??(props.refinementSetups?.length===1?props.refinementSetups[0]:undefined);
  const [importedTasks, setImportedTasks] = useState<string[] | null>(null);
  const [message, setMessage] = useState(""), [projectError, setProjectError] = useState("");
  const blocked = loading || props.busy || !props.enabled;
  const canWrite = !blocked && !stale && overview?.mode === "ready" && !pending;

  async function run(action: (current: () => boolean) => Promise<void>) {
    if (lock.current || !latest.current.enabled) return;
    lock.current = true; const token = ++generation.current;
    const current = () => alive.current && generation.current === token;
    setLoading(true); setMessage("");
    try { await action(current); }
    catch (error) { if (current()) setMessage(String(error)); }
    finally { if (current()) { lock.current = false; setLoading(false); } }
  }
  async function refresh(current: () => boolean) {
    const replies = await Promise.allSettled([
      latest.current.read({ type: "overview" }),
      latest.current.read({ type: "plans", offset: 0, revision: null }),
      latest.current.read({ type: "projects", offset: 0, revision: null }),
    ]);
    if (!current()) return;
    const [o, p, j] = replies;
    const overviewOk = o.status === "fulfilled" && o.value.type === "overview";
    const plansOk = p.status === "fulfilled" && p.value.type === "plans";
    if (overviewOk && o.value.type === "overview") { setOverview(o.value.value); latest.current.onOverview(o.value.value); }
    if (plansOk && p.value.type === "plans") { setPlans(p.value.value); setPlanBack([]); setDetail(null); }
    if (j.status === "fulfilled" && j.value.type === "projects") { setProjects(j.value.value); setProjectBack([]); setProjectError(""); }
    else { setProjects(null); setProjectError("项目清单暂不可用，请保留原记录并重新读取。"); }
    setStale(!overviewOk || !plansOk);
    if (!overviewOk || !plansOk) setMessage("部分计划记录尚未刷新，保留上次内容；请重新读取后再操作。");
  }
  useEffect(() => {
    alive.current = true; lock.current = false;
    if (props.enabled) void run(refresh);
    return () => { alive.current = false; generation.current++; lock.current = false; };
  }, [props.enabled]);

  function planPage(offset: number, back: number[]) {
    if (!plans) return;
    const revision = plans.revision;
    void run(async current => {
      try {
        const reply = await latest.current.read({ type: "plans", offset, revision });
        if (!current()) return;
        if (reply.type !== "plans" || reply.value.revision !== revision || reply.value.offset !== offset) throw Error("计划页版本不符，请刷新首页。");
        setPlans(reply.value); setPlanBack(back); setDetail(null);
      } catch (error) { if (current()) setStale(true); throw error; }
    });
  }
  function projectPage(offset: number, back: number[]) {
    if (!projects) return;
    const revision = projects.revision;
    void run(async current => {
      const reply = await latest.current.read({ type: "projects", offset, revision });
      if (!current()) return;
      if (reply.type !== "projects" || reply.value.revision !== revision || reply.value.offset !== offset) throw Error("项目页版本不符，请刷新首页。");
      setProjects(reply.value); setProjectBack(back);
    });
  }
  async function submit(command: Command, uncertain: Pending, current: () => boolean) {
    // Once sent, keep the original form frozen unless acceptance is known.
    setPending(uncertain);
    let accepted = false;
    try { accepted = await latest.current.send(command); }
    catch { /* Result unknown: never resend. */ }
    if (!current()) return;
    if (!accepted) { setMessage("操作结果尚未确认。请读取原记录核对；不会自动再次提交。"); return; }
    setPending(null); setDetail(null);
    if (uncertain.type === "create") { setTitle(""); setTasks(""); setImportedTasks(null); }
    await refresh(current);
    if (current()) setMessage("操作确认已收到。若列表尚未更新，请重新读取；不会重复提交原操作。");
  }
  function checkPending() {
    const original = pending;
    void run(async current => {
      await refresh(current);
      if (!current() || !original) return;
      if (original.type === "create") { setMessage("已请求读取原列表。请按保留的名称和任务核对；名称相同本身不能证明本次创建成功。"); return; }
      const page = await latest.current.read({ type: "plans", offset: 0, revision: null });
      if (page.type !== "plans") throw Error("未取得原计划列表版本。");
      const found = await latest.current.read({ type: "plan", id: original.id, revision: page.value.revision });
      if (!current()) return;
      if (found.type !== "plan" || found.value.id !== original.id || found.value.digest !== original.digest) throw Error("原计划身份或内容不符，继续保留未确认状态。");
      if (found.value.approved) { setPending(null); setMessage("原计划的相同内容已批准；本次仅回查，没有再次提交。"); }
      else setMessage("尚未读到原计划的批准结果，继续保留原操作并核对。");
    });
  }

  return <section className="workspace-browser" aria-label="独立工作区记录">
    <div className="section-heading"><HelpHeading as="h2" topic="WorkspaceBrowser">{props.showPlans ? "本地计划记录" : "项目清单"}</HelpHeading><button className="secondary-button" disabled={blocked} onClick={() => void run(refresh)}>刷新工作区记录</button></div>
    {!props.enabled && <p>请在桌面应用中读取本地记录。</p>}
    {overview?.message && <p role="alert"><ErrorText error={overview.message}/></p>}
    {message && <p role="status">{message}</p>}
    {props.showPlans ? <>
      <section className="panel"><HelpHeading as="h3" topic="WorkspaceBrowser">创建计划</HelpHeading><p className="secondary">这里用于手工记录任务。编程项目请使用主指挥规划与项目执行方案。</p>
        <form onSubmit={e => { e.preventDefault(); if (canWrite) void run(current => submit({ type: "createPlan", title, tasks: importedTasks ?? tasks.split("\n").filter(t => t.trim()) }, { type: "create" }, current)); }}>
          <div className="document-chief-configuration"><label htmlFor="document-refinement-project">文档提炼使用的项目主指挥</label><select id="document-refinement-project" value={chief?.setup.projectId??""} onChange={e=>setRefinementProject(e.target.value)} disabled={!canWrite||!props.refinementSetups?.length}><option value="">{props.refinementSetups?.length?"请选择已配置项目":"尚无已保存的项目配置"}</option>{props.refinementSetups?.map(s=><option key={s.setup.projectId} value={s.setup.projectId}>{s.setup.projectId} · {s.setup.chiefModel}</option>)}</select>
          {!props.refinementSetups?.length&&<p className="secondary">先关联项目目录并保存主指挥配置，才会在此显示。文档自动提炼需使用 API 主指挥；手工填写计划仍可继续。</p>}
          {props.configureChief&&<button type="button" className="secondary-button" disabled={blocked} onClick={()=>props.configureChief?.(chief?.setup.projectId)}>配置项目主指挥</button>}</div>
          <MarkdownImport chief={chief} api={props.refinementApi} mode="plan" contextKey={JSON.stringify([title,tasks])} disabled={!canWrite} onApply={(_text,plan)=>{if(plan){setTitle(plan.title);setTasks(plan.tasks.join("\n\n"));setImportedTasks(plan.tasks);}}}/>
          <label htmlFor="workspace-title">计划名称</label><input id="workspace-title" required maxLength={200} value={title} disabled={blocked || !!pending} onChange={e => setTitle(e.target.value)}/>
          <label htmlFor="workspace-tasks">{importedTasks ? "导入的任务内容，空行分隔" : "具体任务，每行一项"}</label><textarea id="workspace-tasks" required rows={5} value={tasks} disabled={blocked || !!pending} onChange={e => {setTasks(e.target.value);if(importedTasks)setImportedTasks(e.target.value.split(/\n\s*\n/).filter(t=>t.trim()));}}/>
          <button className="primary" disabled={!canWrite}>保存计划</button>
        </form>
      </section>
      {pending && <div className="panel"><p>原操作待核对，输入已保留。</p><button className="secondary-button" disabled={blocked} onClick={checkPending}>读取原记录核对</button><button className="secondary-button" disabled={blocked} onClick={() => { setPending(null); setDetail(null); setStale(true); setMessage("已返回编辑。请刷新并核对原记录，再决定是否进行新的操作。"); }}>已核对，返回编辑</button></div>}
      <HelpHeading as="h3" topic="WorkspaceBrowser">已保存的计划：{plans?.total ?? "尚未读取"}</HelpHeading>
      {!plans ? <p>计划清单尚未取得。</p> : plans.total === 0 ? <p>尚未创建计划。</p> : plans.entries.map(plan => <article className="panel plan-card" key={plan.id}>
        <HelpHeading as="h3" topic="WorkspaceBrowser">{plan.title}</HelpHeading><p>{plan.taskCount} 项任务 · {plan.approved ? "已批准" : "待确认"}</p>
        <button className="secondary-button" disabled={blocked || stale || !!pending} onClick={() => void run(async current => {
          const reply = await latest.current.read({ type: "plan", id: plan.id, revision: plans.revision });
          if (!current()) return;
          if (reply.type !== "plan" || reply.value.id !== plan.id || reply.value.digest !== plan.digest) throw Error("计划详情身份或内容不符，请刷新。");
          setDetail(reply.value);
        })}>查看完整计划</button>
      </article>)}
      {plans && <nav className="workspace-pagination" aria-label="计划分页"><button className="secondary-button" disabled={blocked || stale || !planBack.length} onClick={() => planPage(planBack.at(-1)!, planBack.slice(0, -1))}>上一页</button><span>当前显示 {plans.entries.length} / {plans.total}</span><button className="secondary-button" disabled={blocked || stale || plans.nextOffset === null} onClick={() => planPage(plans.nextOffset!, [...planBack, plans.offset])}>下一页</button></nav>}
      {detail && <section className="panel" role="dialog" aria-label="完整计划审阅"><HelpHeading as="h3" topic="WorkspaceBrowser">{detail.title}</HelpHeading><ol>{detail.tasks.map((task, i) => <li key={i}>{task}</li>)}</ol><p>批准仅记录对当前内容的确认，不派发模型任务。</p><button className="secondary-button" disabled={blocked || !!pending} onClick={() => setDetail(null)}>关闭</button><button className="primary" disabled={!canWrite || detail.approved} onClick={() => void run(current => submit({ type: "approvePlan", id: detail.id, digest: detail.digest }, { type: "approve", id: detail.id, digest: detail.digest }, current))}>{detail.approved ? "已批准" : "确认批准"}</button></section>}
    </> : <>
      {projectError && <p role="alert"><ErrorText error={projectError}/></p>}
      {!projects ? <p>项目清单尚未取得。</p> : projects.total === 0 ? <p>尚未登记项目。</p> : projects.entries.map(project => <article className="panel workspace-project-card" key={project.project.id}><HelpHeading as="h3" topic="WorkspaceBrowser">{project.project.id}</HelpHeading><p>{project.project.root}</p><p>{project.taskCount} 项任务；{project.resourceHoldingTasks} 项仍占用资源，其中 {project.unknownTasks} 项结果未知。</p></article>)}
      {projects && <nav className="workspace-pagination" aria-label="项目分页"><button className="secondary-button" disabled={blocked || !projectBack.length} onClick={() => projectPage(projectBack.at(-1)!, projectBack.slice(0, -1))}>上一页</button><span>当前显示 {projects.entries.length} / {projects.total}</span><button className="secondary-button" disabled={blocked || projects.nextOffset === null} onClick={() => projectPage(projects.nextOffset!, [...projectBack, projects.offset])}>下一页</button></nav>}
    </>}
  </section>;
}
