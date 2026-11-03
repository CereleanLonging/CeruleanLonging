import { useState } from "react";
import { managementKeyMissing } from "./ManagementProviders";
import type { Command, ExecutionSnapshot, Snapshot } from "./contracts";
const labels = {
  active: "持续调度中",
  paused: "已暂停后续调度",
  attention: "需要处理",
  completed: "任务执行已完成",
  superseded: "已转入修订",
};
type Review = {
  projectId: string;
  revisionDigest: string;
  setupDigest: string;
  root: string;
  chief: string;
  deputy: string;
  limit: number;
};
function cancellationBlock(
  data: Snapshot,
  workflow: ExecutionSnapshot,
): string | null {
  if(workflow.revision?.execution.some(e=>e.adapterId==="workbuddy"))return "WorkBuddy 支持已移除，请修订方案并选择其他执行器。";
  const sources = data.projectSetups.find(s => s.setup.projectId === workflow.project.id)?.setup.managementSources;
  if (managementKeyMissing(data,sources?.chief)) return "请先配置所选主指挥供应商的本机凭证。";
  if (managementKeyMissing(data,sources?.deputy)) return "请先配置所选副指挥供应商的本机凭证。";
  const desktops=workflow.revision?.execution
    .filter(e=>["pending","assigned","unknown","cancelling"].includes(workflow.state.tasks[e.taskId]?.state??""))
    .flatMap(e=>e.desktopWorker?[e.desktopWorker]:[])??[];
  if(desktops.length){
    for(const desktop of desktops){
      const window=desktop.binding.observation.window;
      if(!(window.app==="qoder"&&window.version==="0.4.3.0"))return "当前桌面应用或版本尚未接入编程调度，需要先完成适配。";
      if(desktop.binding.bridgeSha256!==data.desktopAccess?.bridgeSha256)return "桌面桥接版本变化，需要重新配置和批准。";
    }
    if(Object.values(workflow.state.tasks).some(t=>t?.state==="unknown"||t?.state==="cancelling"))return "原桌面任务结果待核对，完成回读或处置后再恢复调度。";
  }
  if(data.apiRecovery)return "API 记录需要恢复，请先等待后台结束并重新校验。";
  const apiWorkers=workflow.revision?.execution.flatMap(execution=>execution.apiWorker?[execution.apiWorker]:[])??[];
  for(const api of apiWorkers){
    const target=api.target;
    const provider=target.kind==="openAi"?"openai":({deepSeek:"deepseek",anthropic:"anthropic",qwenBeijing:"qwen",qwenSingapore:"qwen",qwenHongKong:"qwen",kimiChina:"kimi",grok:"grok",a6Api:"a6api",a6ApiVip:"a6api"} as const)[target.endpoint.kind];
    if(provider==="openai" ? !data.openaiConnection.keyConfigured : !data.providerCredentials?.some(c=>c.provider===provider&&c.keyConfigured))return `请先配置 ${provider} 编程模型凭证。`;
    if(data.apiJobs?.some(j=>j.projectId===workflow.project.id&&["unknown","cancel-requested"].includes(j.state)))return "API 执行或停止结果仍需核对，暂不能恢复调度。";
  }
  if(workflow.state.revisionRequested)return "原方案已进入修订，请批准下一版后明确启动。";
  const tasks = Object.values(workflow.state.tasks).filter((task) => !!task);
  const jobs = data.codexJobs.filter(
    (job) =>
      job.projectId === workflow.project.id &&
      job.revisionDigest === workflow.revisionDigest,
  );
  if (
    jobs.some((job) =>
      ["cancel-requested", "cancelling"].includes(job.state),
    ) ||
    tasks.some((task) => task.state === "cancelling")
  )
    return "正在处理停止请求，暂不能恢复项目调度。请等待执行器结果。";
  if (
    tasks.some(
      (task) => task.state === "unknown" && task.cancellationRequested === true,
    )
  )
    return "停止结果尚未确认，请先核对原会话。完成核对前不能恢复项目调度。";
  if (
    tasks.some((task) => task.state === "cancelled" || task.state === "retired") ||
    jobs.some((job) => job.state === "cancelled")
  )
    return "本方案中的任务已停止，需要重新规划并批准后再继续。";
  return null;
}
export function ProjectRuns({
  data,
  send,
  busy,
}: {
  data: Snapshot | null;
  send: (command: Command) => Promise<boolean>;
  busy: boolean;
}) {
  const [review, setReview] = useState<Review | null>(null);
  const reviewedWorkflow =
    review &&
    data?.workflows.find(
      (workflow) => workflow.project.id === review.projectId,
    );
  const reviewBlock =
    data && reviewedWorkflow ? cancellationBlock(data, reviewedWorkflow) : null;
  const reviewCurrent =
    !!review &&
    !!reviewedWorkflow?.approval &&
    reviewedWorkflow.revisionDigest === review.revisionDigest &&
    data?.projectSetups.find(
      (saved) => saved.setup.projectId === review.projectId,
    )?.digest === review.setupDigest;
  return (
    <section className="workflow-plans" aria-label="项目持续执行">
      <h2>项目持续执行</h2>
      <p className="secondary">
        启动后，核心会按已批准方案调用副指挥、编程模型、独立验收与主指挥收口。桌面任务会等待你审阅并确认发送。关闭窗口后核心仍继续；暂停会阻止后续调度，已发送任务继续收集结果。
      </p>
      {data?.workflows
        .filter((w) => w.approval)
        .map((w) => {
          const saved = data.projectSetups.find(
            (s) => s.setup.projectId === w.project.id,
          );
          const run = data.projectRuns?.find(
            (r) =>
              r.projectId === w.project.id &&
              r.revisionDigest === w.revisionDigest,
          );
          const active = run?.state === "active";
          const blocked = cancellationBlock(data, w);
          return (
            <article className="panel" key={w.project.id}>
              <div className="section-heading">
                <h3>{w.project.root.split(/[\\/]/).at(-1)}</h3>
                <span className="badge">
                  {run ? labels[run.state] : "已批准，尚未启动"}
                </span>
              </div>
              {run && <p>{run.message}</p>}
              {active&&w.revision?.execution.some(e=>!!e.desktopWorker)&&Object.values(w.state.tasks).some(t=>t?.state==="assigned")&&<p>副指挥已分配；请在桌面编程任务中准备、审阅并确认发送。等待期间不会自动发送或再次请求分配。</p>}
              {!saved && (
                <p className="secondary">当前方案尚未绑定项目运行配置。</p>
              )}
              {blocked && <p role="status">{blocked}</p>}
              {active ? (
                <button
                  className="secondary-button"
                  disabled={busy}
                  onClick={() =>
                    void send({ type: "pauseProjectRun", runId: run.id })
                  }
                >
                  暂停后续调度
                </button>
              ) : (
                run?.state !== "completed" && run?.state !== "superseded" && (
                  <button
                    className="primary"
                    disabled={
                      busy ||
                      !!blocked ||
                      !saved ||
                      !w.revisionDigest
                    }
                    onClick={() => {
                      if (!blocked && saved && w.revisionDigest)
                        setReview({
                          projectId: w.project.id,
                          revisionDigest: w.revisionDigest,
                          setupDigest: saved.digest,
                          root: w.project.root,
                          chief: saved.setup.chiefModel,
                          deputy: saved.setup.deputyModel,
                          limit: saved.setup.managementMaxRequests,
                        });
                    }}
                  >
                    {run ? "恢复项目调度" : "启动已批准项目"}
                  </button>
                )
              )}
            </article>
          );
        })}
      {review && (
        <div className="modal-backdrop">
          <section
            className="modal panel execution-review"
            role="dialog"
            aria-modal="true"
            aria-labelledby="run-review-title"
          >
            <h2 id="run-review-title">启动持续项目执行</h2>
            <p>{review.root}</p>
            <p>
              主指挥：{review.chief}；副指挥：{review.deputy}。
            </p>
            <p>
              项目主副指挥累计请求上限 {review.limit}{" "}
              次。启动会按方案自动调用模型，API
              费用按供应商实际用量计收，请求次数不是金额上限。
            </p>
            <p>
              任何未知请求、验收失败或指挥要求补充信息都会停下，不会自动重发。
            </p>
            {reviewedWorkflow?.revision?.execution.some(e=>!!e.desktopWorker)&&<p>桌面任务分配后会等待你审阅并确认发送。上一任务独立验收并收口后，后续任务会重新核对聊天历史，每条都需要你确认发送。</p>}
            {reviewBlock && <p role="alert">{reviewBlock}</p>}
            {!reviewCurrent && (
              <p role="alert">项目方案或配置已变化，请返回重新审阅。</p>
            )}
            <div className="card-footer">
              <button
                className="secondary-button"
                onClick={() => setReview(null)}
                disabled={busy}
              >
                返回
              </button>
              <button
                className="primary"
                disabled={busy || !!reviewBlock || !reviewCurrent}
                onClick={async () => {
                  if (
                    !reviewBlock &&
                    reviewCurrent &&
                    (await send({
                      type: "startProjectRun",
                      projectId: review.projectId,
                      revisionDigest: review.revisionDigest,
                      setupDigest: review.setupDigest,
                    }))
                  )
                    setReview(null);
                }}
              >
                确认启动调度
              </button>
            </div>
          </section>
        </div>
      )}
    </section>
  );
}
