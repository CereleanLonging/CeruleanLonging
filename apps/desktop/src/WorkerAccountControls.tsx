import { HelpHeading } from "./HelpHeading";
import type { Command, SavedProjectSetup, Snapshot } from './contracts';

export function WorkerAccountControls({saved,data,disabled,send}:{saved:SavedProjectSetup;data:Snapshot;disabled:boolean;send:(c:Command)=>Promise<boolean>}) {
  if(!saved.setup.chiefDirectory)return null;
  const retired=saved.setup.workerAdapter==='workbuddy'||saved.setup.profiles?.some(p=>p.workerAdapter==='workbuddy');
  const setup=data.workerAccountSetups?.find(s=>s.projectId===saved.setup.projectId);
  const workflow=data.workflows.find(w=>w.project.id===saved.setup.projectId);
  const approved=!!workflow?.approval&&!workflow.state.revisionRequested;
  const started=approved&&(Object.values(workflow.state.tasks).some(t=>!t||t.state!=='pending')||data.projectRuns?.some(r=>r.projectId===saved.setup.projectId&&r.revisionDigest===workflow.revisionDigest));
  const authorized=workflow?.approval?.workerAccountSetup?.setupDigest===saved.digest;
  const names:Record<string,string>={pending:'等待本次 Windows 设置',ready:'账户设置完成；执行前会重新检查权限',failed:'账户设置未完成'};
  const choices=[{id:null,adapter:saved.setup.workerAdapter??'codex',model:saved.setup.worker?.model??saved.setup.desktopWorker?.modelLabel},...(saved.setup.profiles??[]).map(p=>({id:p.id,adapter:p.workerAdapter,model:p.worker?.model??p.desktopWorker?.modelLabel}))].filter(p=>['codex','qoder'].includes(p.adapter));
  return <section className="panel" aria-label="执行账户隔离">
    <HelpHeading as="h3" topic="WorkerAccountControls">Windows 执行账户</HelpHeading>
    <p>目录隔离项目的独立客观检查，以及 CLI 和桌面接入，使用各自绑定的普通账户。可先手动设置，或在审阅方案时勾选授权，由工作台自动发起一次准备。尚无管理员权限时，Windows 会请求 UAC。系统与公开运行依赖仍可读取。登录凭证分别保存，不复制当前用户的登录。</p>
    {authorized&&!setup&&<p role="status">方案已记录管理授权，尚未取得准备回执。不会自动补发；核对后可明确点击下方按钮发起一次。</p>}
    {setup&&<p role="status">{names[setup.state]??setup.state}{setup.message&&`：${setup.message}`}</p>}
    <button className="secondary-button" type="button" disabled={disabled||started||retired||(approved&&!authorized)||setup?.state==='pending'} onClick={()=>void send({type:'provisionWorkerAccounts',projectId:saved.setup.projectId,setupDigest:saved.digest})}>设置或补齐独立账户</button>
    {choices.map(p=><button key={p.id??'primary'} type="button" disabled={disabled||started||setup?.state!=='ready'} onClick={()=>void send({type:'openWorkerLogin',projectId:saved.setup.projectId,setupDigest:saved.digest,profileId:p.id})}>打开 {p.adapter} · {p.model} 独立{p.adapter==='codex'?'登录':'应用'}</button>)}
    <p className="secondary">桌面应用须在独立窗口登录，核对对应工作目录和模型后重新关联聊天。无法核对账户或目录时不会发送任务。取消 UAC 后不会自动重弹；再次点击设置表示重新发起一次。</p>
  </section>;
}
