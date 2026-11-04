import { HelpHeading } from "./HelpHeading";
import { useEffect, useRef, useState } from 'react';
import type { Command, DesktopWorkerSelection, Snapshot } from './contracts';
import { DesktopSessions } from './DesktopSessions';

export function DesktopWorkerFields({data,projectId,app,worker,onChange,idPrefix="planning-desktop",isolationEnabled=false,send,busy=false}:{busy?:boolean;send?:(command:Command)=>Promise<boolean>;isolationEnabled?:boolean;idPrefix?:string;data:Snapshot;projectId:string;app:'qoder';worker:DesktopWorkerSelection|undefined|null;onChange:(worker:DesktopWorkerSelection|undefined)=>void}) {
  const [connecting, setConnecting] = useState(false);
  const bindingSelect = useRef<HTMLSelectElement>(null);
  useEffect(() => setConnecting(false), [projectId]);
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
  },[project?.root,worker?.binding.id,worker?.modelLabel,worker?.projectRoot,isolationEnabled,app]);
  const bindings=data.desktopAccess?.bindings.filter(b=>b.projectId===projectId&&b.observation.window.app===app&&b.observation.modelLabel)??[];
  return <section className="panel" aria-label="桌面编程会话配置">
    <HelpHeading as="h3" topic="DesktopWorkerFields">已有聊天与执行范围</HelpHeading>
    <p>选择当前项目已关联的 Qoder 聊天。首次使用时，打开并登录 Qoder，选择模型与聊天，再点击下方按钮读取并关联。</p>
    {isolationEnabled&&<p className="secondary">项目配置保存后，在 Windows 执行账户中准备独立账户，登录该账户的 Qoder 并重新关联聊天，核对分配的执行目录。</p>}
    {send && <button type="button" className="secondary-button" disabled={busy||!!data.desktopAccess?.checking} aria-expanded={connecting} aria-controls={`${idPrefix}-connection`} onClick={() => setConnecting(value => !value)}>
      {connecting ? "收起 Qoder 会话关联" : "关联 Qoder 会话"}
    </button>}
    {connecting && send && <div id={`${idPrefix}-connection`} className="desktop-inline-connection">
      <DesktopSessions data={data} busy={busy} send={send} projectId={projectId} idPrefix={`${idPrefix}-session`}
        onBound={() => { setConnecting(false); requestAnimationFrame(() => bindingSelect.current?.focus()); }}/>
    </div>}
    <label htmlFor={`${idPrefix}-binding`}>选择已保存会话</label>
    <select ref={bindingSelect} id={`${idPrefix}-binding`} required value={worker?.binding.id??''} onChange={e=>{const binding=bindings.find(b=>b.id===e.target.value);onChange(binding&&project?{binding,projectRoot:project.root,modelLabel:binding.observation.modelLabel!,workspaceConfirmed:false,externalToolsAcknowledged:false}:undefined);}}>
      <option value="">选择已观察模型的会话关联</option>
      {bindings.map(b=><option key={b.id} value={b.id}>{b.label} · {b.observation.modelLabel} · {b.observation.window.version}</option>)}
    </select>
    {!bindings.length&&<p role="status">当前项目还没有包含模型名称的 Qoder 会话关联。请{send ? "点击“关联 Qoder 会话”" : "在接入管理中"}发现窗口、读取当前聊天，并确认保存项目关联。</p>}
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
