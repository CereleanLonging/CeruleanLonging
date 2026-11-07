import {ObjectiveTestReview} from "./ObjectiveTestReview";
import { HelpHeading } from "./HelpHeading";
import { effectiveSupervision, supervisionSummary } from "./SupervisionFields";
import { CurrencyBudgetReview } from "./CurrencyBudgetFields";
import { DesktopExecutionReview } from "./DesktopWorkerFields";
import { ApiExecutionReview } from "./ApiExecutionReview";
import { NativeWorkerCost } from "./NativeWorkerCost";
import type { PlanRevision, ToolPermission } from "./contracts";
const permissions: Record<ToolPermission, string> = {
  "read-files": "读取项目文件", "write-files": "修改项目文件", "run-commands": "运行命令",
  network: "访问网络", "desktop-send": "发送桌面会话消息",
};
export function ExecutionAuthorization({revision}:{revision:PlanRevision}) {
  return <>
      <dl><dt>项目目录</dt><dd>{revision.project.root}</dd><dt>方案版本</dt><dd>{revision.workflow.version}</dd>
        <dt>允许操作</dt><dd>{revision.policy.tools.map(p => permissions[p]).join("、")}</dd>
        <dt>调用与时长上限</dt><dd>{revision.policy.maxRequests} 次 · {revision.policy.maxSeconds} 秒</dd>
        {revision.managementSetupDigest && <><dt>主、副指挥配置</dt><dd>本方案已绑定保存时的会话、来源和模型；更换后须重新规划与批准。<details><summary>配置摘要</summary><code>{revision.managementSetupDigest}</code></details></dd></>}
        {revision.worktree && <><dt>任务工作目录</dt><dd>每次尝试使用独立 Git worktree，基准提交 <code>{revision.worktree.baseCommit}</code>。{revision.worktree.input === "committed-raw-blobs-with-accepted-dependencies" ? "在该提交上合入已接受前置任务的封存修改和删除，冲突时暂停。" : "输入仅含该提交的原始文件。"}不包含项目目录未提交的修改，也不执行 Git LFS 或其他检出转换。</dd><dt>工作目录存放位置</dt><dd>{revision.worktree.poolRoot}</dd><dt>Git 程序</dt><dd>{revision.worktree.git.path} · SHA-256 <code>{revision.worktree.git.sha256}</code></dd></>}
        <dt>项目默认异常监督</dt><dd>{supervisionSummary(revision.policy.supervision)} 各任务的最终策略如下。</dd>
        <dt>费用上限</dt><dd>{revision.policy.currencyBudget ? <CurrencyBudgetReview value={revision.policy.currencyBudget}/> : revision.policy.budgetUsdCents == null ? "未设置金额上限" : `$${(revision.policy.budgetUsdCents / 100).toFixed(2)}`}</dd>
          <dt>验收项目副本</dt><dd>{revision.snapshotPolicy ? `包含${revision.worktree ? "本次尝试工作目录" : "项目目录"}中除排除项外的全部文件；最多 ${revision.snapshotPolicy.maxFiles} 个文件、${revision.snapshotPolicy.maxBytes.toLocaleString()} 字节` : "旧方案尚未配置受测副本，需要更新后才能执行验收"}</dd>
        {revision.snapshotPolicy && <><dt>副本排除项</dt><dd>{revision.snapshotPolicy.exclude.join("、") || "无"}</dd></>}
      </dl>
      {revision.workflow.tasks.map(task => {
        const execution = revision.execution.find(e => e.taskId === task.id)!;
        return <div key={task.id}><HelpHeading as="h3" topic="ExecutionAuthorization">{task.goal}</HelpHeading>{revision.worktree?.integration?.taskId===task.id&&<p><strong>项目最终集成</strong>：在独立目录合入所有前置成果，下列整体检查及主指挥收口均通过后才完成项目。</p>}<p>执行接入：{execution.adapterId} · 执行配置：{execution.profileId??"默认配置"}</p><p>批准的共享资源：{task.resourceKeys?.length?task.resourceKeys.join("、"):"未声明"}</p>
          <p>此任务异常监督（{execution.supervisionOverride ? "单独设置" : "沿用项目默认"}）：{supervisionSummary(effectiveSupervision(execution.supervisionOverride,revision.policy.supervision))}</p>
          {execution.worker && <dl><dt>执行模型</dt><dd>{execution.worker.provider} / {execution.worker.model}</dd><dt>执行程序</dt><dd><code>{execution.worker.executable.path}</code></dd>{execution.worker.ompRuntimeParent&&<><dt>OMP 独立运行目录</dt><dd><code>{execution.worker.ompRuntimeParent}</code></dd></>}<dt>执行程序 SHA-256</dt><dd><code>{execution.worker.executable.sha256}</code></dd></dl>}
          {execution.desktopWorker && <DesktopExecutionReview worker={execution.desktopWorker}/>}
          {execution.apiWorker && <ApiExecutionReview worker={execution.apiWorker}/>}
          <NativeWorkerCost adapter={execution.adapterId}/>
          <p>前置任务：{task.dependencies.join("、") || "无"}</p>
          {execution.workerDirectory&&<p>绑定的模型工作目录：<code>{revision.project.root}\{execution.workerDirectory.relativePath.replaceAll("/","\\")}</code>。运行中不会切换到其他模型目录；CLI／桌面需要对应的独立 Windows 账户。</p>}
          <p>交付文件：{execution.artifactPaths.join("、")}</p>
          <p>按顺序完成全部验收检查：</p><ol>{execution.checks.map(check => <li key={check.id}><strong>{check.name??check.id}</strong>{!!check.expectedAssertions?.length&&<p>必须通过的断言：{check.expectedAssertions.join("、")}</p>} · <ObjectiveTestReview check={check}/>（最多 {check.timeoutSeconds} 秒）
            {check.tool ? <dl><dt>实际执行程序</dt><dd><code>{check.tool.executable.path}</code></dd><dt>程序 SHA-256</dt><dd><code>{check.tool.executable.sha256}</code></dd>
              <dt>固定验收文件</dt><dd>{check.tool.inputs.length ? check.tool.inputs.map(file=><div key={file.path}><code>{file.path}</code><br/><code>{file.sha256}</code></div>) : "无附加文件"}</dd>
              <dt>验收环境设置</dt><dd>{Object.entries(check.tool.environment).map(([key,value])=>`${key}=${value}`).join("、") || "使用核心隔离环境，不继承凭证或 PATH"}</dd></dl>
              : <p className="secondary">旧方案未绑定实际程序，需更新方案后才能运行验收。</p>}
          </li>)}</ol>
        </div>;
      })}
  </>;
}
