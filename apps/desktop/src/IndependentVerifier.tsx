import {invoke} from "@tauri-apps/api/core";
import {useEffect,useState} from "react";
import {ErrorText} from "./ErrorText";
type Status={ready:boolean;version:string;path:string;sha256:string|null;error:string|null};
export function IndependentVerifier({connected}:{connected:boolean}) {
  const [status,setStatus]=useState<Status|null>(null);
  const [error,setError]=useState("");
  const [refresh,setRefresh]=useState(0);
  useEffect(()=>{
    let current=true;setStatus(null);setError("");
    if(connected)void invoke<Status>("independent_verifier_status").then(value=>{if(current)setStatus(value);},reason=>{if(current)setError(String(reason));});
    return()=>{current=false;};
  },[connected,refresh]);
  return <div className="independent-verifier" aria-label="绑定的独立验收工具">
    <label htmlFor="workbench-verifier">默认独立验收工具</label>
    <select id="workbench-verifier" disabled value="workbench-verifier"><option value="workbench-verifier">CeruleanLonging 独立验收工具 · 固定关联</option></select>
    <p className="secondary">随工作台构建的独立程序，负责源码脱敏分片、票据核验和全票判定。工作台保留授权、联网转发及最终确认；执行模型不能更换此工具。</p>
    <p role="status">{!connected?"等待本地核心连接。":!status&&!error?"正在核对独立验收工具…":status?.ready?`已绑定 · ${status.version} · 程序摘要匹配`:"独立验收工具未就绪，不能开始盲评。"}</p>
    {(error||status?.error)&&<p role="alert"><ErrorText error={error||status!.error!}/></p>}
    <p className="secondary">绑定工具不会自动联网或启用盲评。项目仍需有效的功能检查和隔离配置；盲评不能代替客观测试。</p>
    <button type="button" className="secondary-button" disabled={!connected} onClick={()=>setRefresh(n=>n+1)}>重新核对绑定</button>
    {status?.ready&&<details><summary>工具位置与版本摘要</summary><p><code>{status.path}</code></p><p><code>{status.sha256}</code></p></details>}
  </div>;
}
