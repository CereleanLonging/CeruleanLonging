import type { Command, SavedProjectSetup, Snapshot } from './contracts';

export function WorkerAccountControls({saved,data,disabled,send}:{saved:SavedProjectSetup;data:Snapshot;disabled:boolean;send:(c:Command)=>Promise<boolean>}) {
  if(!saved.setup.chiefDirectory)return null;
  const setup=data.workerAccountSetups?.find(s=>s.projectId===saved.setup.projectId);
  const names:Record<string,string>={pending:'等待本次 Windows 设置',ready:'账户设置完成；执行前会重新检查权限',failed:'账户设置未完成'};
  const choices=[{id:null,adapter:saved.setup.workerAdapter??'codex',model:saved.setup.worker?.model??saved.setup.desktopWorker?.modelLabel},...(saved.setup.profiles??[]).map(p=>({id:p.id,adapter:p.workerAdapter,model:p.worker?.model??p.desktopWorker?.modelLabel}))].filter(p=>['codex','qoder','workbuddy'].includes(p.adapter));
  return <section className="panel" aria-label="执行账户隔离">
    <h3>Windows 执行账户</h3>
    <p>CLI 和桌面接入使用各自的普通账户。设置会请求 UAC，保护主指挥资料、核心数据和其他模型目录。系统与公开运行依赖仍可读取。登录凭证分别保存，不复制当前用户的登录。</p>
    {setup&&<p role="status">{names[setup.state]??setup.state}{setup.message&&`：${setup.message}`}</p>}
    <button className="secondary-button" type="button" disabled={disabled||setup?.state==='pending'} onClick={()=>void send({type:'provisionWorkerAccounts',projectId:saved.setup.projectId,setupDigest:saved.digest})}>设置或补齐独立账户</button>
    {choices.map(p=><button key={p.id??'primary'} type="button" disabled={disabled||setup?.state!=='ready'} onClick={()=>void send({type:'openWorkerLogin',projectId:saved.setup.projectId,setupDigest:saved.digest,profileId:p.id})}>打开 {p.adapter} · {p.model} 独立{p.adapter==='codex'?'登录':'应用'}</button>)}
    <p className="secondary">桌面应用须在独立窗口登录，核对对应工作目录和模型后重新关联聊天。无法核对账户或目录时不会发送任务。取消 UAC 后不会自动重弹；再次点击设置表示重新发起一次。</p>
  </section>;
}
