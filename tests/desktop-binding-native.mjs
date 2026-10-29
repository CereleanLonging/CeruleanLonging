// Native product UI/IPC/persistence with synthetic external-window observations.
import {chromium} from 'playwright-core';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join,toNamespacedPath} from 'node:path';
import assert from 'node:assert/strict';
import {assertNativeRuntime} from './native-runtime.mjs';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');await assertNativeRuntime();
const report=await mkdtemp(resolve('reports/desktop-binding-native-'));
const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{windowsHide:true,encoding:'utf8',timeout:120000});assert.equal(build.status,0,build.stderr);
const exe=build.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(exe);
const server=spawn(exe,['--exact','host::desktop_sessions::tests::native_desktop_binding_server','--ignored','--nocapture'],{windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,CERULEAN_DESKTOP_NATIVE_DIR:report}});
let serverLog='';server.stdout.on('data',b=>serverLog+=b);server.stderr.on('data',b=>serverLog+=b);
const sleep=ms=>new Promise(r=>setTimeout(r,ms));async function until(fn,label){for(let i=0;i<300;i++){const v=await fn();if(v)return v;await sleep(100);}throw Error('Timed out: '+label);}
let app,browser,page,data;const errors=[];const request=command=>page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command);
async function open(){
  const socket=createServer().listen(0,'127.0.0.1');await once(socket,'listening');const port=socket.address().port;await new Promise(r=>socket.close(r));
  app=spawn(resolve('target/debug/cerulean-desktop.exe'),[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(report,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
  await until(async()=>{assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});return true;}catch{return false;}},'WebView');
  page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')),'page');page.on('pageerror',e=>errors.push(e.message));
  await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});const snap=await request({type:'snapshot'});assert.equal(toNamespacedPath(resolve(snap.dataDirectory)),toNamespacedPath(resolve(data)),'isolated fixture only');
  await page.getByRole('button',{name:'接入管理',exact:false}).click();
}
async function close(){const exit=once(app,'exit');await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});await Promise.race([exit,new Promise((_,reject)=>setTimeout(()=>reject(Error('window exit timeout')),8000).unref())]);await browser.close();browser=null;const stop=spawnSync(resolve('target/debug/cerulean-core.exe'),['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:25000});assert.equal(stop.status,0,stop.stderr);}
try{
  const fixture=await until(async()=>{assert.equal(server.exitCode,null,serverLog);try{return JSON.parse(await readFile(join(report,'fixture.json'),'utf8'));}catch{return false;}},'fixture');data=fixture.data;await sleep(300);await open();
  const region=page.getByRole('region',{name:'已有桌面会话',exact:true});
  await region.getByRole('button',{name:'发现已打开窗口',exact:true}).click();
  for(const appName of ['Qoder','WorkBuddy']) {
    const row=region.getByRole('article').filter({has:page.getByRole('heading',{name:appName+' · 工作台接入验证',exact:true})});
    await row.getByRole('button',{name:'读取当前会话',exact:true}).click();
    await region.getByRole('button',{name:'审阅项目关联',exact:true}).click();
    const dialog=page.getByRole('dialog',{name:'确认已有会话关联',exact:true});await dialog.waitFor();
    const save=dialog.getByRole('button',{name:'重新核对并保存关联',exact:true});assert.equal(await save.isDisabled(),true);
    await dialog.locator('#desktop-label').fill(appName+' 专用验证聊天');await dialog.getByRole('checkbox').check();
    await dialog.screenshot({path:join(report,appName.toLowerCase()+'-review.png')});await save.click();await dialog.waitFor({state:'hidden'});
    await region.getByText(`已保存的会话关联（${appName==='Qoder'?1:2}）`,{exact:true}).waitFor({timeout:20000});
  }
  const before=await request({type:'snapshot'});assert.equal(before.desktopAccess.bindings.length,2);assert.equal(before.projectRuns.length,0);assert.equal(before.apiJobs.length,0);
  assert.equal(before.desktopAccess.bindings.find(b=>b.observation.window.app==='workbuddy').observation.sessionId,null);
  await close();await until(()=>server.exitCode!==null,'fixture service exit');assert.equal(server.exitCode,0,serverLog);
  // Reopen with the actual production core, which contains no observation fixture.
  await open();const after=await request({type:'snapshot'});assert.deepEqual(after.desktopAccess.bindings,before.desktopAccess.bindings);assert.equal(after.desktopAccess.observation,null);assert.equal(after.desktopAccess.checking,false);assert.equal(after.desktopAccess.windows.length,0);
  const restored=page.getByRole('region',{name:'已有桌面会话',exact:true});await restored.getByText('已保存的会话关联（2）',{exact:true}).click();await restored.screenshot({path:join(report,'restored.png')});assert.deepEqual(errors,[]);
  let liveQoderRead=false;const liveAppsRead=[];
  if(process.env.CERULEAN_NATIVE_LIVE_DESKTOP_REPORT) {
    const liveWindows=JSON.parse(await readFile(process.env.CERULEAN_NATIVE_LIVE_DESKTOP_REPORT,'utf8')).windows.filter(w=>['qoder','workbuddy'].includes(w.window.app)&&w.observation);
    assert.ok(liveWindows.length,'prior read-only application identity required');
    for(const live of liveWindows) {
    await restored.getByRole('button',{name:'发现已打开窗口',exact:true}).click();
    const discovered=await until(async()=>{const s=await request({type:'snapshot'});if(s.desktopAccess.error)throw Error(s.desktopAccess.error);return !s.desktopAccess.checking&&s.desktopAccess.windows.some(w=>w.hwnd===live.window.hwnd)?s:false;},'actual discovered window');
    assert.ok(discovered.desktopAccess.windows.some(w=>w.hwnd===live.window.hwnd&&w.pid===live.window.pid&&w.startTicks===live.window.startTicks&&w.exe===live.window.exe&&w.version===live.window.version));
    const label=live.window.app==='qoder'?'Qoder':'WorkBuddy';
    const actual=restored.getByRole('article').filter({has:page.getByRole('heading',{name:label+' · '+live.window.title,exact:true})});await actual.waitFor({timeout:20000});assert.equal(await actual.count(),1);
    await actual.getByRole('button',{name:'读取当前会话',exact:true}).click();await restored.getByRole('button',{name:'审阅项目关联',exact:true}).waitFor({timeout:30000});
    const observed=await until(async()=>{const s=await request({type:'snapshot'});if(s.desktopAccess.error)throw Error(s.desktopAccess.error);return !s.desktopAccess.checking&&s.desktopAccess.observation?.window.hwnd===live.window.hwnd?s:false;},'actual observation for '+label);assert.equal(observed.desktopAccess.observation.sessionId,live.observation.sessionId);assert.equal(observed.desktopAccess.bindings.length,2,'never bind the unrelated current business chat');
    assert.equal(observed.desktopAccess.observation.modelLabel??null,live.observation.modelLabel??null,'model display survives production IPC');
    await restored.getByRole('article',{name:'当前会话观察',exact:true}).screenshot({path:join(report,'live-'+live.window.app+'-observation.png')});liveAppsRead.push(live.window.app);if(live.window.app==='qoder')liveQoderRead=true;
    }
  }
  await close();await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',scope:'actual Tauri/IPC user binding with synthetic observations; production-core reopen and optional real application read only',apps:['qoder','workbuddy'],nativeDialogs:2,confirmationRequired:true,workbuddyHasNoSupplierSessionId:true,reopenPreservesHistoryOnly:true,liveQoderRead,liveAppsRead,externalMessages:0,modelCalls:0,pageErrors:errors},null,2));console.log(JSON.stringify({report,status:'passed'}));
}catch(error){await page?.screenshot({path:join(report,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(report,'failure.json'),JSON.stringify({error:String(error),errors,serverLog},null,2));throw error;}
finally{await writeFile(join(report,'service.log'),serverLog);await browser?.close().catch(()=>{});for(const child of [app,server])if(child&&child.exitCode===null){const exit=once(child,'exit');child.kill();await exit;}}
