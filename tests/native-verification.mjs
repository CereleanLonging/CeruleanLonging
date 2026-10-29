import {chromium} from "playwright-core";
import {spawn,spawnSync} from "node:child_process";
import {once} from "node:events";
import {createServer} from "node:net";
import {mkdtemp,readFile,writeFile} from "node:fs/promises";
import {join,resolve} from "node:path";
import assert from "node:assert/strict";
const dir=await mkdtemp(resolve("reports/native-verification-"));
const reconcile=process.argv.includes("--reconcile");
const build=spawnSync("cargo",["test","-p","cerulean-core","--test","strict_workflow","--no-run","--locked","--message-format=json"],{windowsHide:true,encoding:"utf8",timeout:60000});
assert.equal(build.status,0,build.stderr);
const artifact=build.stdout.split(/\r?\n/).filter(Boolean).map(line=>JSON.parse(line)).find(message=>message.reason==="compiler-artifact" && message.target.name==="strict_workflow" && message.executable);
assert.ok(artifact);
let owner,app,browser,page;const errors=[];
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
try {
  owner=spawn(artifact.executable,["--ignored","--exact","native_async_service_fixture","--nocapture"],{windowsHide:true,stdio:"ignore",env:{...process.env,CERULEAN_NATIVE_ASYNC_DIR:dir,...(reconcile?{CERULEAN_NATIVE_RECONCILE:"1"}:{})}});
  let fixture;
  for(let n=0;n<100;n++){
    assert.equal(owner.exitCode,null,"fixture remains alive");
    try{fixture=JSON.parse(await readFile(join(dir,"fixture-service.json"),"utf8"));break;}catch{await delay(50);}
  }
  assert.ok(fixture,"fixture service ready");
  const listener=createServer();listener.listen(0,"127.0.0.1");await once(listener,"listening");const port=listener.address().port;await new Promise(r=>listener.close(r));
  app=spawn(resolve("target/debug/cerulean-desktop.exe"),[],{windowsHide:true,stdio:"ignore",env:{...process.env,CERULEAN_TEST_DATA_DIR:fixture.dataDirectory,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
  for(let n=0;n<80;n++){try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:1000});break;}catch{assert.equal(app.exitCode,null);await delay(100);}}
  assert.ok(browser);
  for(let n=0;n<50;n++){page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes("tauri.localhost"));if(page)break;await delay(100);}
  assert.ok(page);page.on("pageerror",e=>errors.push(e.message));
  await page.getByText("本地核心已连接",{exact:true}).waitFor({timeout:10000});
  await page.getByRole("button",{name:"需求与计划",exact:false}).click();
  if(reconcile){
    await page.getByText("验收待对账",{exact:true}).waitFor();
    await page.getByRole("button",{name:"处理待对账验收",exact:true}).click();
    const review=page.getByRole("dialog");
    await review.getByRole("heading",{name:"放弃本次验收",exact:true}).waitFor();
    assert.equal(await review.getByRole("button",{name:"确认放弃本次验收",exact:true}).isDisabled(),true);
    await review.getByLabel("放弃原因",{exact:true}).fill("验收中断，保留原始记录，放弃本次结果。");
    await page.screenshot({path:join(dir,"reconciliation-review.png"),fullPage:true});
    await review.getByRole("button",{name:"确认放弃本次验收",exact:true}).click();
    await page.getByText("本次验收已放弃",{exact:true}).waitFor();
    await page.screenshot({path:join(dir,"abandoned.png"),fullPage:true});
    const base=join(fixture.dataDirectory,"workflows/project/verification/interrupted-check");
    const audit=JSON.parse(await readFile(join(base,"abandonment.json"),"utf8"));
    assert.equal(audit.userId,"user");assert.equal(audit.operationId,"interrupted-check");assert.equal(audit.reason,"验收中断，保留原始记录，放弃本次结果。");
    assert.equal(await readFile(join(base,"intent.json"),"utf8"),"{fixture interrupted before result}");
  } else {
  await page.getByText("验收进行中",{exact:true}).waitFor();
  await page.getByRole("button",{name:"停止验收",exact:true}).scrollIntoViewIfNeeded();
  await page.screenshot({path:join(dir,"running.png"),fullPage:true});
  await page.getByRole("button",{name:"停止验收",exact:true}).click();
  await page.getByText("验收未通过",{exact:true}).waitFor({timeout:10000});
  await page.screenshot({path:join(dir,"cancelled.png"),fullPage:true});
  const job=JSON.parse(await readFile(join(fixture.dataDirectory,"verification-jobs/project/native-check.json"),"utf8"));
  assert.equal(job.state,"failed");assert.match(job.message,/cancelled/);
  }
  const query=spawnSync(resolve("target/debug/cerulean-core.exe"),["--request","--data-dir",fixture.dataDirectory],{windowsHide:true,encoding:"utf8",timeout:10000,input:JSON.stringify({id:"verify-ui-cancel",protocolVersion:1,command:{type:"snapshot"}})});
  assert.equal(query.status,0,query.stderr);const snapshot=JSON.parse(query.stdout).response.result;
  assert.equal(snapshot.workflows[0].state.tasks.task.verified,false);
  if(reconcile)assert.equal(snapshot.verificationJobs[0].state,"abandoned");
  assert.deepEqual(errors,[]);
  const exiting=once(app,"exit");await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke("plugin:window|close",{label:"main"})).catch(()=>{});
  await Promise.race([exiting,new Promise((_,reject)=>setTimeout(()=>reject(Error("window close timed out")),8000).unref())]);app=null;
  const report={at:new Date().toISOString(),status:"passed",dataDirectory:fixture.dataDirectory,checks:reconcile?{nativeUnknownBadge:true,explicitReasonRequired:true,nativeAbandonClick:true,persistedUserAudit:true,originalEvidencePreserved:true,taskNotVerified:true,noPageErrors:true,normalWindowClose:true}:{nativeRunningBadge:true,nativeCancelClick:true,automaticTerminalRefresh:true,persistedCancellation:true,taskNotVerified:true,noPageErrors:true,normalWindowClose:true},modelCalls:0};
  await writeFile(join(dir,"result.json"),JSON.stringify(report,null,2));console.log(JSON.stringify({dir,...report},null,2));
} catch(error) {
  await page?.screenshot({path:join(dir,"failure.png"),fullPage:true}).catch(()=>{});
  await writeFile(join(dir,"failure.json"),JSON.stringify({error:String(error),errors},null,2));throw error;
} finally {
  await browser?.close().catch(()=>{});app?.kill();
  if(owner && owner.exitCode===null){const done=once(owner,"exit");owner.kill();await done;}
}
