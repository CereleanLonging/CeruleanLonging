import { ErrorText } from "./ErrorText";
import { useState } from "react";
import type { Command, RegisteredValidator } from "./contracts";

export function ValidatorCatalog({items,send,busy,connected}:{items:RegisteredValidator[];send:(command:Command)=>Promise<boolean>;busy:boolean;connected:boolean}) {
  const [id,setId]=useState(()=>`validator-${crypto.randomUUID()}`);
  const [name,setName]=useState("");
  const [executable,setExecutable]=useState("");
  const [inputs,setInputs]=useState("");
  const [environment,setEnvironment]=useState("");
  const [error,setError]=useState("");
  return <section className="panel validator-catalog" aria-label="验收工具登记">
    <h2>验收工具</h2>
    <p className="secondary">登记已有程序和固定验收文件。这里只读取文件并保存版本；实际任务使用前会在执行方案中再次审阅。</p>
    <form onSubmit={async event=>{
      event.preventDefault();setError("");
      const env:Record<string,string>={};
      for(const line of environment.split(/\r?\n/).map(line=>line.trim()).filter(Boolean)) {
        const match=/^([A-Z_]+)=(.*)$/.exec(line);
        if(!match || Object.hasOwn(env,match[1])) {setError("环境设置请按每行一个 NAME=value 填写，名称不能重复。");return;}
        env[match[1]]=match[2];
      }
      if(await send({type:"registerValidator",id,name:name.trim(),executable:executable.trim(),inputs:inputs.split(/\r?\n/).map(line=>line.trim()).filter(Boolean),environment:env})) {
        setId(`validator-${crypto.randomUUID()}`);setName("");setExecutable("");setInputs("");setEnvironment("");
      }
    }}>
      <label htmlFor="validator-name">验收工具名称</label><input id="validator-name" required maxLength={100} value={name} onChange={event=>setName(event.target.value)} placeholder="例如：项目测试工具 v1" />
      <label htmlFor="validator-executable">程序完整路径</label><input id="validator-executable" required value={executable} onChange={event=>setExecutable(event.target.value)} placeholder="D:\\tools\\validator.exe" />
      <label htmlFor="validator-inputs">固定验收文件（可选，每行一个完整路径）</label><textarea id="validator-inputs" value={inputs} onChange={event=>setInputs(event.target.value)} />
      <label htmlFor="validator-environment">环境设置（可选）</label><textarea id="validator-environment" value={environment} onChange={event=>setEnvironment(event.target.value)} placeholder={"CI=true\nNO_COLOR=1"} />
      <p className="secondary">允许 CI、NO_COLOR、PYTHONNOUSERSITE、PYTHONDONTWRITEBYTECODE、RUST_BACKTRACE、CARGO_NET_OFFLINE、NPM_CONFIG_OFFLINE。固定文件最多 16 个，须在项目目录之外。</p>
      {error && <p role="alert"><ErrorText error={error}/></p>}
      <button className="primary" disabled={busy || !connected}>读取文件并登记</button>
    </form>
    <h3>已登记版本 · {items.length}</h3>
    {items.length===0 ? <p className="secondary">尚未登记验收工具。</p> : items.map(item=><article key={item.id} className="validator-entry">
      <h4>{item.name}</h4><p className="secondary">{new Date(item.registeredAt).toLocaleString("zh-CN")} · 登记时文件摘要，运行前再次核验</p>
      <dl><dt>实际程序</dt><dd><code>{item.tool.executable.path}</code></dd><dt>程序 SHA-256</dt><dd><code>{item.tool.executable.sha256}</code></dd>
      <dt>固定验收文件</dt><dd>{item.tool.inputs.length ? item.tool.inputs.map(file=><div key={file.path}><code>{file.path}</code><br/><code>{file.sha256}</code></div>) : "无"}</dd>
      <dt>环境设置</dt><dd>{Object.entries(item.tool.environment).map(([key,value])=>`${key}=${value}`).join("、") || "核心隔离环境"}</dd></dl>
      <p className="secondary">登记号：{item.id}。升级时另建登记，已批准方案保留原版本。</p>
    </article>)}
  </section>;
}
