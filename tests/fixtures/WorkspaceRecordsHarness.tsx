import { useEffect, useRef, useState } from "react";
import { WorkspaceTasks } from "../../apps/desktop/src/WorkspaceTasks";
import { WorkspaceExecution } from "../../apps/desktop/src/WorkspaceExecution";
import { WorkspaceJobs } from "../../apps/desktop/src/WorkspaceJobs";
import { WorkspaceVerificationRechecks } from "../../apps/desktop/src/WorkspaceVerificationRechecks";
import { WorkspaceRecovery } from "../../apps/desktop/src/WorkspaceRecovery";
import { WorkspaceConfigurations } from "../../apps/desktop/src/WorkspaceConfigurations";
import { WorkspaceHistory } from "../../apps/desktop/src/WorkspaceHistory";
import { WorkspaceCodexStartups } from "../../apps/desktop/src/WorkspaceCodexStartups";
import { WorkspaceManagementDecisions } from "../../apps/desktop/src/WorkspaceManagementDecisions";
import { WorkspaceProjectRuns } from "../../apps/desktop/src/WorkspaceProjectRuns";
import { WorkspaceRevisionReviews } from "../../apps/desktop/src/WorkspaceRevisionReviews";
import { WorkspaceUnsentAssignments } from "../../apps/desktop/src/WorkspaceUnsentAssignments";
import { WorkspaceCodexCancellations } from "../../apps/desktop/src/WorkspaceCodexCancellations";
import type { Command, ProjectPage, WorkspaceQuery, WorkspaceResult } from "../../apps/desktop/src/contracts";

// Test-only selection host for retained record components. These entry points
// no longer appear in the shipped workbench. No application code imports this.
type Props = { read: (query: WorkspaceQuery) => Promise<WorkspaceResult>; send: (command: Command) => Promise<boolean>; enabled: boolean; busy: boolean; showPlans: boolean; onOverview: unknown };
export function WorkspaceRecordsHarness(props: Props) {
  const [projects, setProjects] = useState<ProjectPage | null>(null);
  const [loading, setLoading] = useState(true);
  const latest=useRef(props); latest.current=props;
  useEffect(()=>{let live=true;void latest.current.read({type:"projects",offset:0,revision:null}).then(r=>{if(live&&r.type==="projects")setProjects(r.value);}).finally(()=>{if(live)setLoading(false);});return()=>{live=false;};},[]);
  const [selectedProject, setSelectedProject] = useState<string | null>(null);
  const [projectRunsProject, setProjectRunsProject] = useState<string | null>(null);
  const [revisionReviewsProject, setRevisionReviewsProject] = useState<string | null>(null);
  const [unsentAssignmentsProject, setUnsentAssignmentsProject] = useState<string | null>(null);
  const [codexCancellationsProject, setCodexCancellationsProject] = useState<string | null>(null);
  const [jobProject, setJobProject] = useState<string | null>(null);
  const [recheckProject, setRecheckProject] = useState<string | null>(null);
  const [recoveryProject, setRecoveryProject] = useState<string | null>(null);
  const [historyProject, setHistoryProject] = useState<string | null>(null);
  const [startupProject, setStartupProject] = useState<string | null>(null);
  const [managementProject, setManagementProject] = useState<string | null>(null);
  const [showConfigurations, setShowConfigurations] = useState(false);
  const [executionReview, setExecutionReview] = useState<{projectId:string;digest:string|null}|null>(null);

  const blocked=loading||props.busy||!props.enabled;
  return <section>
    <button disabled={blocked}>刷新工作区记录</button>
    <button disabled={blocked} onClick={()=>setShowConfigurations(v=>!v)}>{showConfigurations?"关闭已保存项目配置":"查看已保存项目配置"}</button>
    {showConfigurations&&<WorkspaceConfigurations read={props.read} enabled={props.enabled} busy={props.busy}/>}
      {!projects ? <p>项目清单尚未取得。</p> : projects.total === 0 ? <p>尚未登记项目。</p> : projects.entries.map(project => <article className="panel workspace-project-card" key={project.project.id}><h3>{project.project.id}</h3><p>{project.project.root}</p><p>{project.taskCount} 项任务；{project.resourceHoldingTasks} 项仍占用资源，其中 {project.unknownTasks} 项结果未知。</p><button className="secondary-button" disabled={blocked} onClick={() => setSelectedProject(project.project.id)}>查看项目 {project.project.id} 的任务</button><button className="secondary-button" disabled={blocked} onClick={() => setJobProject(project.project.id)}>查看项目 {project.project.id} 的作业</button><button className="secondary-button" disabled={blocked} onClick={() => setProjectRunsProject(project.project.id)}>查看项目 {project.project.id} 的运行记录</button><button className="secondary-button" disabled={blocked} onClick={() => setExecutionReview({projectId:project.project.id,digest:project.revisionDigest})}>审阅项目 {project.project.id} 的执行授权</button></article>)}
      {projects?.entries.map(p => <button className="secondary-button" key={p.project.id} disabled={blocked} onClick={() => setStartupProject(p.project.id)}>查看项目 {p.project.id} 的编程启动记录</button>)}
      {startupProject && <><button className="secondary-button" onClick={() => setStartupProject(null)}>关闭项目启动记录</button><WorkspaceCodexStartups key={startupProject} projectId={startupProject} read={props.read} busy={blocked || !props.enabled}/></>}
      {projects?.entries.map(p => <button className="secondary-button" key={p.project.id} disabled={blocked} onClick={() => setManagementProject(p.project.id)}>查看项目 {p.project.id} 的管理决策</button>)}
      {managementProject && <><button className="secondary-button" onClick={() => setManagementProject(null)}>关闭项目管理决策</button><WorkspaceManagementDecisions key={managementProject} projectId={managementProject} read={props.read} busy={blocked}/></>}
      {selectedProject && <><button className="secondary-button" onClick={() => setSelectedProject(null)}>关闭项目任务</button><WorkspaceTasks projectId={selectedProject} read={props.read} busy={blocked}/></>}
      {jobProject && <><button className="secondary-button" onClick={() => setJobProject(null)}>关闭项目作业</button><WorkspaceJobs projectId={jobProject} read={props.read} send={props.send} busy={blocked} onTaskEvidence={() => setSelectedProject(jobProject)}/></>}
      {projectRunsProject && <><button className="secondary-button" onClick={() => setProjectRunsProject(null)}>关闭项目运行记录</button><WorkspaceProjectRuns key={projectRunsProject} projectId={projectRunsProject} read={props.read} busy={blocked}/></>}
      {projects?.entries.map(p => <button className="secondary-button" key={p.project.id} disabled={blocked} onClick={() => setRevisionReviewsProject(p.project.id)}>查看项目 {p.project.id} 的修订审阅</button>)}
      {revisionReviewsProject && <><button className="secondary-button" onClick={() => setRevisionReviewsProject(null)}>关闭项目修订审阅</button><WorkspaceRevisionReviews key={revisionReviewsProject} projectId={revisionReviewsProject} read={props.read} busy={blocked}/></>}
      {projects?.entries.map(p => <button className="secondary-button" key={p.project.id} disabled={blocked} onClick={() => setUnsentAssignmentsProject(p.project.id)}>查看项目 {p.project.id} 的未发送分配审阅</button>)}
      {unsentAssignmentsProject && <><button className="secondary-button" onClick={() => setUnsentAssignmentsProject(null)}>关闭项目未发送分配审阅</button><WorkspaceUnsentAssignments key={unsentAssignmentsProject} projectId={unsentAssignmentsProject} read={props.read} busy={blocked}/></>}
      {projects?.entries.map(p => <button className="secondary-button" key={p.project.id} disabled={blocked} onClick={() => setCodexCancellationsProject(p.project.id)}>查看项目 {p.project.id} 的 Codex 取消审阅</button>)}
      {codexCancellationsProject && <><button className="secondary-button" onClick={() => setCodexCancellationsProject(null)}>关闭项目 Codex 取消审阅</button><WorkspaceCodexCancellations key={codexCancellationsProject} projectId={codexCancellationsProject} read={props.read} busy={blocked}/></>}
      {projects?.entries.map(p => <button className="secondary-button" key={p.project.id} disabled={blocked} onClick={() => setRecoveryProject(p.project.id)}>查看项目 {p.project.id} 的恢复记录</button>)}
      {recoveryProject && <><button className="secondary-button" onClick={() => setRecoveryProject(null)}>关闭项目恢复记录</button><WorkspaceRecovery projectId={recoveryProject} read={props.read} busy={blocked}/></>}
      {projects?.entries.map(p => <button className="secondary-button" key={p.project.id} disabled={blocked} onClick={() => setHistoryProject(p.project.id)}>查看项目 {p.project.id} 的历史</button>)}
      {historyProject && <><button className="secondary-button" onClick={() => setHistoryProject(null)}>关闭项目历史</button><WorkspaceHistory key={historyProject} projectId={historyProject} read={props.read} busy={blocked}/></>}
      {executionReview && <WorkspaceExecution projectId={executionReview.projectId} expectedDigest={executionReview.digest} read={props.read} send={props.send} busy={blocked} close={() => setExecutionReview(null)}/>}
      {projects?.entries.map(p => <button className="secondary-button" key={p.project.id} disabled={blocked} onClick={() => setRecheckProject(p.project.id)}>查看项目 {p.project.id} 的复验记录</button>)}
      {recheckProject && <><button className="secondary-button" onClick={() => setRecheckProject(null)}>关闭项目复验记录</button><WorkspaceVerificationRechecks key={recheckProject} projectId={recheckProject} read={props.read} send={props.send} busy={blocked}/></>}
  </section>;
}
