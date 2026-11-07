import { HelpHeading } from "./HelpHeading";
import type { ProjectSetupDraft, ProjectWorkerAdapter } from "./contracts";

const adapters: Record<ProjectWorkerAdapter, string> = {
  codex: "Codex CLI（历史配置）", omp: "OMP · API", qoder: "Qoder 桌面",
  workbuddy: "WorkBuddy（已移除）", openai: "OpenAI API", anthropic: "Claude API",
  deepseek: "DeepSeek API", qwen: "Qwen API", kimi: "Kimi API", grok: "Grok API", a6api: "中转 API",
};
type Worker = Pick<ProjectSetupDraft, "workerAdapter" | "workerModel" | "apiWorker" | "desktopWorker">;

function WorkerSummary({ label, worker }: { label: string; worker: Worker }) {
  const desktop = worker.workerAdapter === "qoder" || worker.workerAdapter === "workbuddy";
  const selection = desktop ? worker.desktopWorker : undefined;
  return <li>
    <strong>{label} · {adapters[worker.workerAdapter ?? "codex"]}</strong>
    <p>模型：{selection?.modelLabel || worker.workerModel || "未选择"}</p>
    {desktop && (selection ? <>
      <p className="desktop-bound-directory">会话 ID：<code>{selection.binding.observation.sessionId || "历史关联未记录会话 ID"}</code></p>
      <p className="desktop-bound-directory">执行目录：<code>{selection.projectRoot}</code></p>
      <p className="secondary">{selection.workspaceConfirmed && selection.externalToolsAcknowledged
        ? "已保存会话关联与范围确认；执行前仍会重新核对会话和模型。"
        : "已保存会话关联；请在接入管理确认当前执行目录与工具范围。"}</p>
    </> : <p role="status">此执行配置尚无有效的桌面会话关联，请在接入管理核对。</p>)}
  </li>;
}

export function ProjectAccessSummary({ draft, accessReady, disabled, onConnections }: {
  draft: ProjectSetupDraft; accessReady: boolean; disabled: boolean; onConnections?: () => void;
}) {
  return <section className="panel" aria-label="已选角色模型">
    <HelpHeading as="h3" topic="ProjectPlanning">接入配置摘要</HelpHeading>
    <p>主指挥：{draft.chiefModel || "未选择"} · 副指挥：{draft.deputyModel || "未选择"}</p>
    {draft.managementSources?.chief.kind === "codexSession" && <p className="secondary desktop-bound-directory">Codex 主指挥会话：{draft.managementSources.chief.sessionId || "待填写"}</p>}
    <ul className="project-worker-summary" aria-label="本项目执行端">
      <WorkerSummary label="默认执行端" worker={draft}/>
      {(draft.profiles ?? []).map((profile, index) => <WorkerSummary key={index} label={`附加执行端 · ${profile.id}`} worker={profile}/>)}
    </ul>
    {accessReady ? <p className="secondary">已沿用接入管理的角色配置，可直接填写下方需求。保存完整配置后再请求主指挥规划。</p>
      : <p role="status">请先在接入管理确认本项目的角色模型，再保存需求与计划。</p>}
    <button className="secondary-button" type="button" disabled={disabled} onClick={onConnections}>修改接入配置</button>
  </section>;
}
