import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import {useEffect,useRef,useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import {PortableEvidence} from "./PortableEvidence";
import {BuildCacheMonitor} from "./BuildCacheMonitor";
type Info={version:string;dataDirectory:string;installationDirectory:string;holdPresent:boolean;bundleSchema:number};
type Session={directory:string;sessionDigest:string;pid:number;started:boolean};
export function Maintenance({desktop}:{desktop:boolean}){
  const [info,setInfo]=useState<Info|null>(null);
  const [session,setSession]=useState<Session|null>(null);
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState("");
  const [logsOpening,setLogsOpening]=useState(false);
  const logsLock=useRef(false);
  const [logsError,setLogsError]=useState("");
  useEffect(()=>{if(desktop)void invoke<Info>("maintenance_info").then(setInfo).catch(e=>setError(String(e)));},[desktop]);
  async function open(){
    setBusy(true);setError("");
    try{setSession(await invoke<Session>("open_maintenance_window"));}
    catch(e){setError(String(e));}
    finally{setBusy(false);}
  }
  async function openLogs(){
    if(!desktop||logsLock.current)return;
    logsLock.current=true;setLogsOpening(true);setLogsError("");
    try{await invoke("open_review_logs_window");}
    catch(e){setLogsError(String(e));}
    finally{logsLock.current=false;setLogsOpening(false);}
  }
  return <section className="panel maintenance-page" aria-label="维护与升级">
    <div className="eyebrow">LOCAL MAINTENANCE</div><HelpHeading as="h1" topic="Maintenance">维护与升级</HelpHeading>
    <section className="panel maintenance-info" aria-label="当前安装信息">
      <HelpHeading as="h2" topic="Maintenance">当前安装信息</HelpHeading>
      <dl className="maintenance-facts">
        <div><dt>当前版本</dt><dd>{info?.version??"等待读取"}</dd></div>
        <div><dt>升级锁定</dt><dd>{info?(info.holdPresent?"已锁定，请读取原事务后恢复":"当前未锁定"):"尚未读取"}</dd></div>
        <div className="maintenance-path"><dt>安装目录</dt><dd><code>{info?.installationDirectory??"桌面运行时显示"}</code></dd></div>
        <div className="maintenance-path"><dt>数据目录</dt><dd><code>{info?.dataDirectory??"桌面运行时显示"}</code></dd></div>
      </dl>
    </section>
    <div className="maintenance-actions-grid">
    <section className="panel maintenance-action-card" aria-label="升级与恢复入口">
      <HelpHeading as="h2" topic="Maintenance">升级与恢复</HelpHeading>
      <p>在独立维护窗口审阅升级、备份和程序更新，并按需进入数据迁移与恢复。</p>
      <ol className="maintenance-steps">
        <li>审阅核心状态、原安装包和新安装包。</li>
        <li>确认锁定并准备备份，程序替换前关闭工作台窗口。</li>
        <li>核对结果并提交，发生中断时读取原事务继续恢复。</li>
      </ol>
      <div className="maintenance-card-action"><button className="primary" disabled={!desktop||busy||!info} onClick={()=>void open()}>{busy?"正在准备维护副本…":"打开独立维护窗口"}</button></div>
      <p className="secondary">维护窗口在安装目录外运行，关闭工作台后仍可继续使用。</p>
      {!desktop&&<p>请在桌面工作台中打开本地维护窗口。</p>}
      {error&&<p role="alert"><ErrorText error={error}/></p>}
      {session&&<div role="status"><p>已启动维护窗口进程 {session.pid}，请在该窗口继续。</p><details><summary>本次维护副本与恢复信息</summary><p>{session.directory}</p><p>会话摘要 {session.sessionDigest}</p><p>操作记录保存在副本的 operations 目录，可用于中断后核对。</p></details></div>}
    </section>
    <section className="panel maintenance-action-card" aria-label="只读日志入口">
      <HelpHeading as="h2" topic="Maintenance">只读日志</HelpHeading>
      <p>在独立窗口查看全局 API 中断复核记录与全局管理复核记录，支持刷新、翻页和查看原记录详情。</p>
      <div className="maintenance-card-action"><button className="secondary-button" disabled={!desktop||logsOpening} onClick={()=>void openLogs()}>{logsOpening?"正在打开日志窗口…":"打开只读日志窗口"}</button></div>
      {logsError&&<p role="alert"><ErrorText error={logsError}/></p>}
    </section>
    </div>
    <PortableEvidence desktop={desktop}/>
    <BuildCacheMonitor desktop={desktop}/>
  </section>;
}
