// Read-only diagnostic fallback when the desktop usage tool is unavailable.
// This script NEVER shuts down the computer or consumes reset credits.
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { writeFile } from "node:fs/promises";
import { isAbsolute } from "node:path";
import { once } from "node:events";
const [executable,reportPath]=process.argv.slice(2);
if(!executable||!isAbsolute(executable)||!reportPath)throw Error("absolute Codex executable and report path required");
const child=spawn(executable,["app-server","--listen","stdio://"],{windowsHide:true,stdio:["pipe","pipe","ignore"]});
const lines=createInterface({input:child.stdout});let timer;
const send=value=>child.stdin.write(JSON.stringify(value)+"\n");
const window=value=>value&&Number.isFinite(value.usedPercent)?{usedPercent:value.usedPercent,windowDurationMins:value.windowDurationMins??null,resetsAt:value.resetsAt??null}:null;
const bucket=value=>({limitId:value.limitId??null,normalModelSlug:value.normalModelSlug??null,primary:window(value.primary),secondary:window(value.secondary),rateLimitReachedType:value.rateLimitReachedType??null,spendControlReached:value.spendControlReached??null,hasCredits:value.credits?.hasCredits??null,unlimited:value.credits?.unlimited??null});
try {
  const result=await new Promise((resolve,reject)=>{
    timer=setTimeout(()=>reject(Error("usage read timed out")),30000);
    child.once("error",()=>reject(Error("Codex launch failed")));
    child.once("exit",()=>reject(Error("Codex exited before usage reply")));
    lines.on("line",line=>{
      if(line.length>4*1024*1024){reject(Error("usage frame too large"));return;}
      let value;try{value=JSON.parse(line);}catch{reject(Error("invalid usage frame"));return;}
      if(value.method&&value.id!==undefined){send({id:value.id,error:{code:-32601,message:"read-only diagnostic"}});return;}
      if(value.id===1){if(value.error){reject(Error("initialization rejected"));return;}send({method:"initialized",params:{}});send({id:2,method:"account/rateLimits/read",params:{excludeResetCreditDetails:true,supportsLunaReserve:false}});}
      if(value.id===2){if(value.error){reject(Error("usage read rejected"));return;}const source=value.result;const map=source.rateLimitsByLimitId;resolve({at:new Date().toISOString(),source:"local Codex CLI login; account match to desktop not independently proven",modelCalls:0,rateLimits:source.rateLimits?bucket(source.rateLimits):null,rateLimitsByLimitId:map?Object.fromEntries(Object.entries(map).map(([k,v])=>[k,bucket(v)])):null});}
    });
    send({id:1,method:"initialize",params:{clientInfo:{name:"cerulean_usage_read",version:"0.1.0"}}});
  });
  await writeFile(reportPath,JSON.stringify(result,null,2));console.log(JSON.stringify(result,null,2));
}finally{
  clearTimeout(timer);lines.close();child.stdin.end();
  if(child.exitCode===null){const exited=once(child,"exit");await Promise.race([exited,new Promise(r=>setTimeout(r,2000).unref())]);if(child.exitCode===null){child.kill();await exited;}}
}
