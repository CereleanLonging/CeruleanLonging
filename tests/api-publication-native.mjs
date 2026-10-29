// Actual Tauri/WebView -> IPC -> diagnostic service -> durable API review and restart.
import {chromium} from 'playwright-core';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdtemp,readFile,writeFile,readdir} from 'node:fs/promises';
import {resolve,join,toNamespacedPath} from 'node:path';
import assert from 'node:assert/strict';
import {assertNativeRuntime} from './native-runtime.mjs';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
await assertNativeRuntime();
const report=await mkdtemp(resolve('reports/api-publication-native-'));
const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{windowsHide:true,encoding:'utf8',timeout:120000});
assert.equal(build.status,0,build.stderr);
const testExe=build.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(testExe);
const desktop=resolve('target/debug/cerulean-desktop.exe'),core=resolve('target/debug/cerulean-core.exe'),results=[];
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,label){for(let n=0;n<180;n++){const value=await fn();if(value)return value;await sleep(100);}throw Error('Timed out: '+label);}
for(const kind of ['startup','cancel']) {
  const dir=await mkdtemp(join(report,kind+'-'));
  const seeded=spawnSync(testExe,['--exact','host::api_publication_tests::native_api_publication_fixture','--ignored','--nocapture'],{windowsHide:true,encoding:'utf8',timeout:30000,env:{...process.env,CERULEAN_API_PUBLICATION_NATIVE_DIR:dir,CERULEAN_API_PUBLICATION_KIND:kind}});
  assert.equal(seeded.status,0,seeded.stdout+seeded.stderr);
  const fixture=JSON.parse(await readFile(join(dir,'fixture.json'),'utf8')),data=fixture.data;
  const original=await readFile(join(data,fixture.relativePath));
  const eventBytes=async()=>{const folder=join(data,'workflows/project/events');return Promise.all((await readdir(folder)).sort().map(async name=>[name,(await readFile(join(folder,name))).toString('base64')]));};
  const beforeEvents=await eventBytes(),errors=[];let app,browser,page;
  const request=command=>page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command);
  try {
    const server=createServer().listen(0,'127.0.0.1');await once(server,'listening');const port=server.address().port;await new Promise(r=>server.close(r));
    app=spawn(desktop,[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(dir,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
    await until(async()=>{assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});return true;}catch{return false;}},'WebView');
    page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')),'page');page.on('pageerror',e=>errors.push(e.message));
    await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});
    const region=page.getByRole('region',{name:'执行数据诊断',exact:true});await region.waitFor();
    const snap=await request({type:'snapshot'});assert.equal(toNamespacedPath(resolve(snap.dataDirectory)),toNamespacedPath(resolve(data)),'must control only the isolated fixture');assert.ok(!JSON.stringify(snap).includes('private-api-publication-marker'));
    const file=snap.coreDiagnostic.apiFiles.find(f=>f.relativePath===fixture.relativePath);assert.equal(file.reviewKind,'api-publication');assert.ok(file.reviewDigest);
    for(const command of [{type:'checkOpenAi'},{type:'reviewApiPublication',relativePath:file.relativePath,evidenceDigest:'0'.repeat(64),reason:'stale'}]) {
      const rejected=await page.evaluate(async command=>{try{await window.__TAURI_INTERNALS__.invoke('core_request',{command});return false;}catch{return true;}},command);assert.equal(rejected,true);
    }
    const row=region.getByRole('article').filter({has:page.getByRole('heading',{name:fixture.relativePath,exact:true})});
    await row.getByRole('button',{name:'复核未写完整的 API 记录…',exact:true}).click();
    const dialog=page.getByRole('dialog',{name:'API 中断记录复核',exact:true});await dialog.waitFor();
    const save=dialog.getByRole('button',{name:'保存 API 复核',exact:true});assert.equal(await save.isDisabled(),true);
    await dialog.getByRole('button',{name:'返回检查',exact:true}).click();
    assert.equal((await request({type:'snapshot'})).coreDiagnostic.apiFiles.find(f=>f.relativePath===file.relativePath).reviewed,false);
    await row.getByRole('button',{name:'复核未写完整的 API 记录…',exact:true}).click();
    await dialog.locator('#api-publication-reason').fill('保留原文和预算，核对执行结果后继续');await dialog.getByRole('checkbox').check();
    await dialog.screenshot({path:join(dir,'review.png')});
    await save.evaluate(button=>{button.click();button.click();});await dialog.waitFor({state:'hidden'});
    const reviewed=await request({type:'snapshot'});assert.ok(reviewed.coreDiagnostic);assert.equal(reviewed.coreDiagnostic.apiFiles.find(f=>f.relativePath===file.relativePath).reviewed,true);
    assert.equal((await readdir(join(data,'api-publication-reviews'))).filter(n=>n.endsWith('.json')).length,1);
    assert.deepEqual(await readFile(join(data,fixture.relativePath)),original);assert.deepEqual(await eventBytes(),beforeEvents);
    await region.getByRole('button',{name:'重新启动并校验',exact:true}).click();await region.waitFor({state:'hidden',timeout:45000});
    const healthy=await request({type:'snapshot'});assert.equal(healthy.coreDiagnostic,null);assert.equal(healthy.apiPublicationReviews.length,1);
    assert.deepEqual(await eventBytes(),beforeEvents);assert.deepEqual(await readFile(join(data,fixture.relativePath)),original);
    const workflow=healthy.workflows.find(w=>w.project.id==='project');assert.equal(workflow.state.tasks.task.state,kind==='cancel'?'unknown':'pending');
    assert.equal(healthy.apiJobs.length,kind==='cancel'?1:0);
    await page.getByRole('button',{name:'需求与计划',exact:false}).click();
    await page.getByText('已复核的 API 中断记录（1）',{exact:true}).waitFor();
    if(kind==='cancel') {
      assert.equal(healthy.apiJobs[0].state,'unknown');assert.ok(healthy.apiJobs[0].retirementDigest);
      await page.getByRole('button',{name:'结束本地未知尝试…',exact:true}).click();
      const retirement=page.getByRole('dialog',{name:'确认结束本地未知尝试',exact:true});await retirement.waitFor();
      await retirement.locator('#api-control-reason').fill('本地结束后修订方案，保留远端不确定性');await retirement.getByRole('checkbox').check();
      await retirement.screenshot({path:join(dir,'retirement.png')});await retirement.getByRole('button',{name:'确认处置',exact:true}).click();await retirement.waitFor({state:'hidden'});
      const ended=await request({type:'snapshot'});assert.equal(ended.apiJobs[0].state,'retired');assert.equal(ended.workflows[0].state.tasks.task.state,'retired');
      const afterEvents=await eventBytes();assert.equal(afterEvents.length,beforeEvents.length+1);assert.deepEqual(afterEvents.slice(0,beforeEvents.length),beforeEvents);
      const event=JSON.parse(Buffer.from(afterEvents.at(-1)[1],'base64').toString('utf8'));assert.equal(event.body.event.command.kind,'retire-unknown-api');
    }
    await page.screenshot({path:join(dir,'recovered.png'),fullPage:true});assert.deepEqual(errors,[]);
    const exit=once(app,'exit');await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});
    await Promise.race([exit,new Promise((_,reject)=>setTimeout(()=>reject(Error('owned window exit timeout')),8000).unref())]);await browser.close();browser=null;
    const stopped=spawnSync(core,['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:25000});assert.equal(stopped.status,0,stopped.stderr);
    results.push({kind,directory:dir,originalBytesPreserved:true,workflowUnchangedByReview:true,staleDigestRejected:true,doubleClickGuard:true,originalRequestNotReplayed:true,localRetirementViaNativeUi:kind==='cancel',gracefulCoreStop:true});
  } catch(error) {await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(error),errors},null,2));throw error;}
  finally {await browser?.close().catch(()=>{});if(app&&app.exitCode===null){const exit=once(app,'exit');app.kill();await exit;}}
}
await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',scope:'actual Tauri WebView, resident IPC, API publication review/restart; synthetic model fixtures',results,externalProviderRequests:0,realModelCalls:0},null,2));
console.log(JSON.stringify({report,status:'passed',cases:results.length}));
