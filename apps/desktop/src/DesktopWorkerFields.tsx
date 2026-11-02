import { useEffect } from 'react';
import type { DesktopWorkerSelection, Snapshot } from './contracts';

export function DesktopWorkerFields({data,projectId,app,worker,onChange,idPrefix="planning-desktop",isolationEnabled=false}:{isolationEnabled?:boolean;idPrefix?:string;data:Snapshot;projectId:string;app:'qoder';worker:DesktopWorkerSelection|undefined|null;onChange:(worker:DesktopWorkerSelection|undefined)=>void}) {
  const project=data.workflows.find(w=>w.project.id===projectId)?.project;
  useEffect(()=>{
    if(!worker||!project)return;
    let active=true;
    async function resolve(){
      let root=project!.root;
      if(isolationEnabled){
        const bytes=new TextEncoder().encode(JSON.stringify([app,null,null,[app,worker!.modelLabel]]));
        const hash=Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',bytes))).map(b=>b.toString(16).padStart(2,'0')).join('');
        root=root.replace(/[\\/]+$/,'')+'\\executors\\'+hash;
      }
      if(active&&root!==worker!.projectRoot)onChange({...worker!,projectRoot:root,workspaceConfirmed:false});
    }
    void resolve();return()=>{active=false;};
  },[project?.root,worker?.modelLabel,isolationEnabled,app]);
  const bindings=data.desktopAccess?.bindings.filter(b=>b.projectId===projectId&&b.observation.window.app===app&&b.observation.modelLabel)??[];
  return <section className="panel" aria-label="桌面编程会话配置">
    <h3>已有聊天与执行范围</h3>{isolationEnabled&&<p>先保存此模型配置并设置独立账户，再打开独立应用登录、核对下方目录，重新关联新账户窗口。保存配置不会发送任务。</p>}
    <p>先在接入管理关联本项目的已有聊天。这里保存会话与权限配置；批准方案并启动调度后，副指挥分配的任务会等待你审阅并确认发送。</p>
    <label htmlFor={`${idPrefix}-binding`}>选择已保存会话</label>
    <select id={`${idPrefix}-binding`} required value={worker?.binding.id??''} onChange={e=>{const binding=bindings.find(b=>b.id===e.target.value);onChange(binding&&project?{binding,projectRoot:project.root,modelLabel:binding.observation.modelLabel!,workspaceConfirmed:false,externalToolsAcknowledged:false}:undefined);}}>
      <option value="">选择已观察模型的会话关联</option>
      {bindings.map(b=><option key={b.id} value={b.id}>{b.label} · {b.observation.modelLabel} · {b.observation.window.version}</option>)}
    </select>
    {!bindings.length&&<p role="status">尚无可用关联。请在接入管理读取目标聊天，确认模型显示名后建立关联；应用重启后需要重新读取。</p>}
    {worker&&<>
      <DesktopExecutionReview worker={worker}/>
      <label className="checkbox-label"><input type="checkbox" required checked={worker.workspaceConfirmed} onChange={e=>onChange({...worker,workspaceConfirmed:e.target.checked})}/>我已在外部应用核对工作目录与模型，确认该聊天用于上面的项目</label>
      <label className="checkbox-label"><input type="checkbox" required checked={worker.externalToolsAcknowledged} onChange={e=>onChange({...worker,externalToolsAcknowledged:e.target.checked})}/>我知悉外部应用沿用自身工具权限，工作台不能拦截其全部文件、命令或网络操作</label>
    </>}
  </section>;
}

export function DesktopExecutionReview({worker}:{worker:DesktopWorkerSelection}) {
  const b=worker.binding,o=b.observation;
  return <section aria-label="桌面执行授权范围">
    <dl><dt>桌面应用</dt><dd>{o.window.app==='qoder'?'Qoder':'WorkBuddy'} {o.window.version}</dd>
      <dt>已关联聊天</dt><dd>{b.label} · {o.window.title}</dd>
      <dt>模型显示名（UI观察）</dt><dd>{worker.modelLabel}</dd>
      <dt>项目目录</dt><dd>{worker.projectRoot}</dd>
      <dt>会话身份证据</dt><dd>{o.sessionId?`Qoder界面路由UUID：${o.sessionId}`:'WorkBuddy内容指纹与用户关联；没有供应商会话ID'}</dd>
      <dt>工具权限</dt><dd>沿用外部应用配置。项目目录由用户核对，当前未由工作台验证外部应用实际工作目录；该关联不提供文件系统沙箱。</dd>
      <dt>费用</dt><dd>桌面订阅或调用费用不可精确计量；计划仅限定请求次数与等待时长，不能保证外部任务在超时后停止。</dd>
    </dl>
    <p className="secondary">执行前仍须核对同一会话、模型、草稿和忙闲状态。更改此关联、模型或目录后，需要重新规划并批准。</p>
    <details><summary>原会话观察依据</summary><p>{new Date(o.capturedAt).toLocaleString('zh-CN')} · {o.window.exe}</p><p>关联摘要：{b.id}</p><p>桥接版本摘要：{b.bridgeSha256}</p></details>
  </section>;
}
