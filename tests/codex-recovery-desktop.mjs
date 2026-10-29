// Real native UI against a COPY of a completed live journal, cut at dispatch intent.
// Read-only remote reconciliation; never turn/start. Original evidence is untouched.
import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:net";
import { mkdir, mkdtemp, readFile, writeFile, readdir, copyFile, realpath } from "node:fs/promises";
import { resolve, join, relative, isAbsolute } from "node:path";
import assert from "node:assert/strict";
const reports=await realpath(resolve("reports"));
const source=await realpath(resolve(process.argv[2]));
const rel=relative(reports,source);
assert.ok(rel && !rel.startsWith("..") && !isAbsolute(rel),"source must be an isolated report directory");
const original=JSON.parse(await readFile(join(source,"result.json"),"utf8"));
assert.equal(original.status,"passed");
const dir=await mkdtemp(join(reports,"native-codex-recovery-"));
const data=join(dir,"data"),ledger=join(data,"workflows/project"),sourceLedger=join(source,"data/workflows/project");
await mkdir(join(ledger,"events"),{recursive:true});
await copyFile(join(sourceLedger,"bindings.json"),join(ledger,"bindings.json"));
let intent=false;
for(const name of (await readdir(join(sourceLedger,"events"))).filter(n=>n.endsWith(".json")).sort()){
  const event=JSON.parse(await readFile(join(sourceLedger,"events",name),"utf8"));
  await copyFile(join(sourceLedger,"events",name),join(ledger,"events",name));
  if(event.body.event.command.kind==="dispatch-intent"){intent=true;break;}
}
assert.ok(intent,"actual dispatch intent required");
await mkdir(join(data,"codex-jobs/project"),{recursive:true});
await copyFile(join(source,"data/codex-jobs/project/resident-attempt.json"),join(data,"codex-jobs/project/resident-attempt.json"));
const server=createServer();server.listen(0,"127.0.0.1");await once(server,"listening");const port=server.address().port;await new Promise(r=>server.close(r));
const core=spawn(resolve("target/debug/cerulean-core.exe"),["--serve","--data-dir",data],{windowsHide:true,stdio:"ignore"});
let app,browser,page;const errors=[];
const request=()=>spawnSync(resolve("target/debug/cerulean-core.exe"),["--request","--data-dir",data],{windowsHide:true,encoding:"utf8",timeout:10000,input:JSON.stringify({id:"snapshot",protocolVersion:1,command:{type:"snapshot"}})});
try {
  for(let i=0;i<80;i++){if(request().status===0)break;assert.equal(core.exitCode,null);await new Promise(r=>setTimeout(r,50));}
  const before=JSON.parse(request().stdout).response.result;
  assert.equal(before.codexJobs[0].state,"unknown");
  app=spawn(resolve("target/debug/cerulean-desktop.exe"),[],{windowsHide:true,stdio:"ignore",env:{...process.env,CERULEAN_TEST_DATA_DIR:data,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
  for(let i=0;i<80;i++){try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:1000});break;}catch{await new Promise(r=>setTimeout(r,200));}}
  assert.ok(browser);
  for(let i=0;i<50;i++){page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes("tauri.localhost"));if(page)break;await new Promise(r=>setTimeout(r,200));}
  assert.ok(page);page.on("pageerror",e=>errors.push(e.message));
  await page.getByText("本地核心已连接",{exact:true}).waitFor({timeout:20000});
  await page.getByRole("button",{name:"需求与计划",exact:false}).click();
  await page.getByText("执行结果待对账",{exact:true}).waitFor();
  await page.screenshot({path:join(dir,"unknown.png"),fullPage:true});
  await page.getByRole("button",{name:"核对原会话",exact:true}).click();
  await page.getByText("正在核对原会话",{exact:true}).waitFor();
  await page.getByText("已提交，待最终验收",{exact:true}).waitFor({timeout:90000});
  await page.screenshot({path:join(dir,"reconciled.png"),fullPage:true});
  const after=JSON.parse(request().stdout).response.result;
  assert.equal(after.codexJobs[0].state,"submitted");
  assert.equal(after.workflows[0].state.tasks.task.verified,false);
  const events=await Promise.all((await readdir(join(ledger,"events"))).filter(n=>n.endsWith(".json")).map(async n=>JSON.parse(await readFile(join(ledger,"events",n),"utf8"))));
  assert.equal(events.filter(e=>e.body.event.command.kind==="dispatch-intent").length,1);
  assert.equal(events.filter(e=>e.body.event.command.kind==="reconcile").length,1);
  const exiting=once(app,"exit");
  await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke("plugin:window|close",{label:"main"})).catch(()=>{});
  await Promise.race([exiting,new Promise((_,reject)=>setTimeout(()=>reject(Error("normal close timed out")),8000).unref())]);
  await browser.close();browser=null;
  assert.equal(JSON.parse(request().stdout).response.result.codexJobs[0].state,"submitted");
  assert.deepEqual(errors,[]);
  const result={status:"passed",at:new Date().toISOString(),source,root:dir,newModelTurns:0,checks:{unknownFromJournal:true,nativeReadOnlyButton:true,asyncRefresh:true,sameAttemptSubmitted:true,noAutoVerification:true,singleOriginalDispatchIntent:true,normalClose:true,residentAfterClose:true,noPageErrors:true}};
  await writeFile(join(dir,"result.json"),JSON.stringify(result,null,2));console.log(JSON.stringify(result,null,2));
}catch(error){await page?.screenshot({path:join(dir,"failure.png"),fullPage:true}).catch(()=>{});await writeFile(join(dir,"failure.json"),JSON.stringify({error:String(error),errors},null,2));throw error;}
finally{await browser?.close().catch(()=>{});if(app?.exitCode===null)app.kill();if(core.exitCode===null){const stopped=once(core,"exit");core.kill();await stopped;}}

