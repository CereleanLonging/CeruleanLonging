import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import {useState} from 'react';
import type {Command,Snapshot} from './contracts';
import { LocalConnectionTest } from './LocalConnectionTest';
export function DesktopSessions({data,busy,send,projectId,idPrefix="desktop",onBound}:{projectId?:string;idPrefix?:string;onBound?:()=>void;data:Snapshot|null;busy:boolean;send:(command:Command)=>Promise<boolean>}) {
  const access=data?.desktopAccess;
  const [project,setProject]=useState('');const [label,setLabel]=useState('');const [ack,setAck]=useState(false);
  const [review,setReview]=useState<string|null>(null);
  const [trialReview,setTrialReview]=useState<string|null>(null);const [trialAck,setTrialAck]=useState(false);
  const trial=access?.trialReview;const unresolved=access?.trials.some(t=>!t.proof);
  const observation=access?.observation?.window.app==='qoder'?access.observation:undefined;
  const disabled=busy||!!access?.checking;
  const current=!!review&&review===access?.observationDigest;
  return <section className="workflow-plans desktop-sessions" aria-label="已有桌面会话">
    <HelpHeading as="h2" topic="DesktopSessions">Qoder 已有会话</HelpHeading>
    <p className="secondary">选择已打开的聊天，读取后关联工作台项目。Qoder 0.4.3.0 可另行确认固定文本验证。批准方案并启动后，副指挥分配的编程任务会在“需求与计划”等待你审阅发送；后续任务会先核对上一条任务及回复，历史或聊天身份变化时停止。</p>
    <button type="button" className="secondary-button" disabled={!access||disabled} onClick={()=>void send({type:'discoverDesktop'})}>发现已打开窗口</button>
    <LocalConnectionTest scope={data?.dataDirectory??""} busy={disabled} request={observation?{kind:"qoder",observation}:null}
      hint="先发现窗口并读取目标会话，再测试当前聊天的读取连接。"/>
    {access?.checking&&<p role="status">正在核对桌面会话或验证消息，请保持目标聊天不变…</p>}
    {access?.error&&<p role="alert"><ErrorText error={access.error}/></p>}
    {access?.windows.filter(window=>window.app==='qoder').map(window=><article className="panel" key={`${window.hwnd}:${window.startTicks}`}>
      <HelpHeading as="h3" topic="DesktopSessions">Qoder · {window.title||'无标题窗口'}</HelpHeading>
      <p className="secondary">版本 {window.version} · 进程 {window.pid}</p>
      <button type="button" className="secondary-button" disabled={disabled} onClick={()=>void send({type:'inspectDesktop',window})}>读取当前会话</button>
    </article>)}
    {observation&&<article className="panel" aria-label="当前会话观察">
      <HelpHeading as="h3" topic="DesktopSessions">{observation.window.title}</HelpHeading>
      <p>模型显示名：{observation.modelLabel??"当前未取得；需补充适配后才能用于编程方案"}</p>
      <p>{observation.sessionId?`Qoder 会话 ID：${observation.sessionId}`:'会话 ID 未取得，请重新核对。'}</p>
      <p className="secondary">读取于 {new Date(observation.capturedAt).toLocaleString('zh-CN')}。未读取到草稿或忙碌控件；这不保证后台没有其他活动。</p>
      <button type="button" className="primary" disabled={disabled} onClick={()=>{setReview(access?.observationDigest??null);setAck(false);setLabel('');setProject(projectId??data?.workflows[0]?.project.id??'');}}>审阅项目关联</button>
    </article>}
    {!!access?.bindings.length&&<details><summary>已保存的会话关联（{access.bindings.length}）</summary>{access.bindings.map(binding=><article className="panel" key={binding.id}>
      <HelpHeading as="h3" topic="DesktopSessions">{binding.label}</HelpHeading><p>{binding.projectId} · {binding.observation.window.app} · {binding.observation.window.title}</p>
      <p className="secondary">模型显示名：{binding.observation.modelLabel??"旧观察未记录，请重新读取并关联"}</p>
      <p className="secondary">保存于 {new Date(binding.confirmedAt).toLocaleString('zh-CN')} · 历史观察，使用前需重新读取核对。</p>
      {binding.observation.window.app==='workbuddy'&&<p>WorkBuddy 支持已移除，此关联仅作历史记录。</p>}
      {binding.observation.window.app==='qoder'&&binding.observation.window.version==='0.4.3.0'&&<button type="button" className="secondary-button" disabled={disabled||unresolved} onClick={()=>void send({type:'prepareDesktopTrial',bindingId:binding.id})}>准备固定消息验证</button>}
    </article>)}</details>}
    {trial&&<article className="panel"><HelpHeading as="h3" topic="DesktopSessions">验证消息已准备</HelpHeading><p>核对目标聊天和完整消息后，再确认发送。准备操作不会发送消息。</p><button type="button" className="primary" disabled={disabled} onClick={()=>{setTrialReview(trial.id);setTrialAck(false);}}>审阅验证消息</button></article>}
    {!!access?.trials.length&&<details open><summary>固定消息验证记录（{access.trials.length}）</summary>{access.trials.map(t=><article className="panel" key={t.review.id}>
      <HelpHeading as="h3" topic="DesktopSessions">{t.proof?'文本往返已核实':'发送结果待核对'}</HelpHeading><p>Qoder · 会话 {t.review.observation.sessionId}</p><p className="secondary">{new Date(t.startedAt).toLocaleString('zh-CN')} · {t.review.marker}</p>
      <p>{t.proof?'已匹配本次消息、完整回复和此前轮次；这不代表编程任务验收通过。':'可能已发送或留下草稿。系统不会重发，也不会另发验证；请保持原聊天，待回复后只读核对。'}</p>
      {t.proof?.observedBaseline&&<p className="secondary">历史核对：{t.proof.observedBaseline.length===t.review.baseline.length?'完整历史':`界面已裁剪，保留的 ${t.proof.observedBaseline.length} 轮与原历史后缀一致`}。</p>}
      {!t.proof&&t.review.bridgeSha256!==access.bridgeSha256&&<p className="secondary">适配器已更新。点击只读核对将使用当前版本检查原记录，并记录本次读取版本；不会改写或重发原消息。</p>}
      {!t.proof&&<button type="button" className="secondary-button" disabled={disabled||!access.bridgeSha256} onClick={()=>void send({type:'readDesktopTrial',trialId:t.review.id,readerSha256:access.bridgeSha256})}>只读核对结果</button>}
    </article>)}</details>}
    {trialReview&&<div className="modal-backdrop"><section className="modal panel execution-review" role="dialog" aria-modal="true" aria-labelledby={`${idPrefix}-trial-title`}>
      <HelpHeading as="h2" topic="DesktopSessions" id={`${idPrefix}-trial-title`}>确认发送验证消息</HelpHeading>
      <p>{trial?.observation.window.title} · Qoder {trial?.observation.window.version}</p><p>会话 ID：{trial?.observation.sessionId}</p>
      <p>将发送以下一条固定消息，可能产生模型费用。消息要求不使用工具，但不能替代外部应用自身的工具权限控制。请保持目标聊天不变。</p>
      <blockquote>{trial?.prompt??'验证准备已失效'}</blockquote>
      <label className="checkbox-label"><input type="checkbox" checked={trialAck} onChange={e=>setTrialAck(e.target.checked)}/>我已核对目标聊天与消息，确认发送这一次验证</label>
      {trial?.id!==trialReview&&<p role="alert">验证内容已变化，请重新准备。</p>}
      <div className="card-footer"><button type="button" className="secondary-button" disabled={disabled} onClick={()=>setTrialReview(null)}>返回</button><button type="button" className="primary" disabled={disabled||!trialAck||trial?.id!==trialReview} onClick={async()=>{if(await send({type:'sendDesktopTrial',reviewDigest:trialReview}))setTrialReview(null);}}>确认发送一次</button></div>
    </section></div>}
    {review&&<div className="modal-backdrop"><section className="modal panel execution-review" role="dialog" aria-modal="true" aria-labelledby={`${idPrefix}-binding-title`}>
      <HelpHeading as="h2" topic="DesktopSessions" id={`${idPrefix}-binding-title`}>确认已有会话关联</HelpHeading>
      <p>{observation?`${observation.window.app==='qoder'?'Qoder':'WorkBuddy'} · ${observation.window.title}`:'原观察已失效'}</p>
      {observation&&<p className="secondary">版本 {observation.window.version} · {observation.window.exe}</p>}
      <p>关联只记录你选择的项目，不验证外部应用的实际工作目录；版本或会话变化后必须重新核对。</p>
      <label htmlFor={`${idPrefix}-project`}>工作台项目</label><select id={`${idPrefix}-project`} value={project} onChange={e=>setProject(e.target.value)}>{data?.workflows.map(w=><option key={w.project.id} value={w.project.id}>{w.project.id} · {w.project.root}</option>)}</select>
      <label htmlFor={`${idPrefix}-label`}>关联名称</label><input id={`${idPrefix}-label`} value={label} maxLength={120} onChange={e=>setLabel(e.target.value)}/>
      <label className="checkbox-label"><input type="checkbox" checked={ack} onChange={e=>setAck(e.target.checked)}/>我确认这是要关联的已有聊天，并已核对项目选择</label>
      {!current&&<p role="alert">观察已变化，请返回重新读取。</p>}
      <div className="card-footer"><button type="button" className="secondary-button" disabled={disabled} onClick={()=>setReview(null)}>返回</button><button type="button" className="primary" disabled={disabled||!current||!project||!label.trim()||!ack} onClick={async()=>{if(current&&await send({type:'bindDesktop',projectId:project,observationDigest:review,label:label.trim()})){setReview(null);onBound?.();}}}>重新核对并保存关联</button></div>
    </section></div>}
  </section>;
}
