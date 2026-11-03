import { ErrorText } from "./ErrorText";
import {useEffect,useRef,useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import {PortableEvidence} from "./PortableEvidence";
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
  return <section className="panel" aria-label="维护与升级">
    <div className="eyebrow">LOCAL MAINTENANCE</div><h1>维护与升级</h1>
    <section className="panel" aria-label="只读日志入口">
      <h2>只读日志</h2>
      <p>在独立窗口查看全局 API 中断复核记录与全局管理复核记录，支持刷新、翻页和查看原记录详情。</p>
      <button className="secondary-button" disabled={!desktop||logsOpening} onClick={()=>void openLogs()}>{logsOpening?"正在打开日志窗口…":"打开只读日志窗口"}</button>
      {logsError&&<p role="alert"><ErrorText error={logsError}/></p>}
    </section>
    <p>独立维护窗口提供升级审阅、备份、程序更新和失败恢复。完成程序升级后，可从该窗口打开数据迁移与恢复，单独审阅数据格式变更。维护窗口在安装目录外运行，关闭工作台后仍可继续使用。</p>
    <dl><dt>当前版本</dt><dd>{info?.version??"等待读取"}</dd><dt>安装目录</dt><dd style={{overflowWrap:"anywhere"}}>{info?.installationDirectory??"桌面运行时显示"}</dd><dt>数据目录</dt><dd style={{overflowWrap:"anywhere"}}>{info?.dataDirectory??"桌面运行时显示"}</dd><dt>升级锁定</dt><dd>{info?(info.holdPresent?"已锁定，请读取原事务后恢复":"当前未锁定"):"尚未读取"}</dd></dl>
    <ol><li>打开维护窗口，审阅核心状态及原、新安装包。</li><li>确认锁定核心并准备备份；执行程序替换前关闭所有工作台窗口。</li><li>核对更新结果后明确提交；遇到中断先读取原事务，按证据恢复。</li></ol>
    <button className="primary" disabled={!desktop||busy||!info} onClick={()=>void open()}>{busy?"正在准备维护副本…":"打开独立维护窗口"}</button>
    {!desktop&&<p>浏览器预览不启动本地维护操作。</p>}
    {error&&<p role="alert"><ErrorText error={error}/></p>}
    {session&&<div role="status"><p>已启动维护窗口进程 {session.pid}。请在该窗口继续；本次打开不会停止核心或执行升级。</p><details><summary>本次维护副本与恢复信息</summary><p style={{overflowWrap:"anywhere"}}>{session.directory}</p><p style={{overflowWrap:"anywhere"}}>会话摘要：{session.sessionDigest}</p><p>操作记录保存在副本的 operations 目录。保留原事务目录和计划摘要，以便中断后核对。</p></details></div>}
    <PortableEvidence desktop={desktop}/>
  </section>;
}
