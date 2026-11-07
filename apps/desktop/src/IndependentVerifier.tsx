import {invoke} from "@tauri-apps/api/core";
import {useEffect,useState} from "react";
import {ErrorText} from "./ErrorText";
type Status={ready:boolean;version:string;path:string;sha256:string|null;error:string|null};
export function IndependentVerifier({connected,checker=false}:{connected:boolean;checker?:boolean}) {
  const title=checker?"独立检查工具":"独立盲评验收工具";
  const [status,setStatus]=useState<Status|null>(null);
  const [error,setError]=useState("");
  const [refresh,setRefresh]=useState(0);
  useEffect(()=>{
    let current=true;setStatus(null);setError("");
    if(connected)void invoke<Status>(checker?"objective_checker_status":"independent_verifier_status").then(value=>{if(current)setStatus(value);},reason=>{if(current)setError(String(reason));});
    return()=>{current=false;};
  },[connected,refresh,checker]);
  return <div className="independent-verifier" aria-label={"绑定的"+title}>
    <label htmlFor={checker?"workbench-checker":"workbench-verifier"}>{title} · 固定关联</label>
    <input id={checker?"workbench-checker":"workbench-verifier"} readOnly value={"CeruleanLonging "+title}/>
    <p className="secondary">{checker?"独立程序执行主指挥提出、用户批准的客观测试，核对真实断言和执行证据，再将报告交给盲评验收工具。缺少测试、运行时或完整结果时不会判为通过。":"独立程序接收检查报告并核验归档，完成源码脱敏分片、评分过程工具核验、票据核验和双权重判定。观察到工具调用，或过程记录无法核实时，该票计零分并判为未通过。"}</p>
    <p role="status">{!connected?"等待本地核心连接。":!status&&!error?`正在核对${title}…`:status?.ready?`已绑定 · ${status.version} · 程序摘要匹配`:`${title}未就绪，请使用完整工作台程序目录。`}</p>
    {(error||status?.error)&&<p role="alert"><ErrorText error={error||status!.error!}/></p>}
    <p className="secondary">绑定工具不会自动联网或启用盲评。项目仍需有效的功能检查和隔离配置；盲评不能代替客观测试。</p>
    <button type="button" className="secondary-button" disabled={!connected} onClick={()=>setRefresh(n=>n+1)}>重新核对绑定</button>
    {status?.ready&&<details><summary>工具位置与版本摘要</summary><p><code>{status.path}</code></p><p><code>{status.sha256}</code></p></details>}
  </div>;
}
