import type { RegisteredValidator, WorktreeSetupDraft } from "./contracts";
import { IntegrationFields } from "./IntegrationFields";

export function WorktreeFields({value, disabled, onChange, validators=[]}: {
  value: WorktreeSetupDraft | undefined;
  disabled: boolean;
  validators?: RegisteredValidator[];
  onChange: (value: WorktreeSetupDraft | undefined) => void;
}) {
  return <section className="panel" aria-label="任务工作目录">
    <h3>任务工作目录</h3>
    <label className="checkbox-label"><input type="checkbox" disabled={disabled} checked={!!value}
      onChange={e => onChange(e.target.checked ? {baseCommit:"", poolRoot:"", gitExecutable:""} : undefined)} />
      为每次编程尝试使用独立 Git 工作目录
    </label>
    {disabled && <p className="secondary">已有桌面聊天使用原项目目录，暂不支持迁入独立工作目录。</p>}
    {value && <>
      <label htmlFor="planning-base-commit">固定基准提交</label>
      <input id="planning-base-commit" required pattern="([0-9a-f]{40}|[0-9a-f]{64})" value={value.baseCommit}
        onChange={e => onChange({...value, baseCommit:e.target.value})} placeholder="完整 Git 提交 ID" />
      <label htmlFor="planning-worktree-pool">任务目录池</label>
      <input id="planning-worktree-pool" required value={value.poolRoot}
        onChange={e => onChange({...value, poolRoot:e.target.value})} placeholder="已存在的专用目录绝对路径" />
      <label htmlFor="planning-worktree-git">Git 程序</label>
      <input id="planning-worktree-git" required value={value.gitExecutable}
        onChange={e => onChange({...value, gitExecutable:e.target.value})} placeholder="git.exe 的绝对路径" />
      <label className="checkbox-label"><input type="checkbox" checked={!!value.inheritDependencies}
        onChange={e => onChange({...value, inheritDependencies:e.target.checked||undefined, integration:e.target.checked?value.integration:undefined})} />
        合入已接受前置任务的封存修改
      </label>
      <p className="secondary">基准使用该提交的原始文件，不包含源目录未提交修改，也不执行 Git LFS 或其他检出转换。保存时固定 Git 程序摘要；开始任务时核验仓库与提交。保存配置不会创建任务目录。</p>
      <p className="secondary">任务仍按顺序执行。启用依赖输入后，在基准上合入前置任务已经接受的修改和删除；互不依赖的任务若产生不同结果，准备会暂停。每个文件最多 1 MiB，总计 256 MiB、4096 个文件；不支持符号链接和子模块。更改配置后需重新规划并批准。</p>
      {value.inheritDependencies&&<IntegrationFields value={value.integration??undefined} validators={validators} onChange={integration=>onChange({...value,integration})}/>}
    </>}
  </section>;
}
