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
const live=process.env.CERULEAN_NATIVE_DESKTOP_LIVE==='1';
const report=await mkdtemp(resolve('reports/desktop-trial-native-'));
const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{windowsHide:true,encoding:'utf8',timeout:120000});assert.equal(build.status,0,build.stderr);
const exe=build.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(exe);
const server=spawn(exe,['--exact',live?'host::desktop_sessions::tests::live_desktop_trial_seed':'host::desktop_sessions::tests::native_desktop_trial_server','--ignored','--nocapture'],{windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,CERULEAN_DESKTOP_NATIVE_DIR:report}});
let serverLog='';server.stdout.on('data',b=>serverLog+=b);server.stderr.on('data',b=>serverLog+=b);
const sleep=ms=>new Promise(r=>setTimeout(r,ms));async function until(fn,label){for(let i=0;i<900;i++){const v=await fn();if(v)return v;await sleep(100);}throw Error('Timed out: '+label);}
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
  const fixture=await until(async()=>{assert.ok(server.exitCode===null||(live&&server.exitCode===0),serverLog);try{return JSON.parse(await readFile(join(report,'fixture.json'),'utf8'));}catch{return false;}},'fixture');data=fixture.data;if(live){await until(()=>server.exitCode!==null,'seed completed');assert.equal(server.exitCode,0,serverLog);}await sleep(300);await open();
  const region=page.getByRole('region',{name:'已有桌面会话',exact:true});
  if(live){
    const target='76edb3bc-078f-4ce4-bc9b-b6525fde9c51';
    await region.getByRole('button',{name:'发现已打开窗口',exact:true}).click();
    const row=region.getByRole('article').filter({has:page.getByRole('heading',{name:'Qoder · Qoder',exact:true})});await row.waitFor({timeout:20000});assert.equal(await row.count(),1);
    await row.getByRole('button',{name:'读取当前会话',exact:true}).click();await region.getByRole('button',{name:'审阅项目关联',exact:true}).waitFor({timeout:25000});
    const observed=await request({type:'snapshot'});assert.equal(observed.desktopAccess.observation.sessionId,target,'only the previously authorized verification chat');assert.equal(observed.desktopAccess.observation.window.version,'0.4.3.0');
    await region.getByRole('button',{name:'审阅项目关联',exact:true}).click();const bindingDialog=page.getByRole('dialog',{name:'确认已有会话关联',exact:true});await bindingDialog.locator('#desktop-label').fill('用户指定的工作台接入验证');await bindingDialog.getByRole('checkbox').check();await bindingDialog.getByRole('button',{name:'重新核对并保存关联',exact:true}).click();await bindingDialog.waitFor({state:'hidden'});
    await region.getByText('已保存的会话关联（1）',{exact:true}).waitFor({timeout:25000});
  }
  await region.getByText('已保存的会话关联（1）',{exact:true}).click();
  await region.getByRole('button',{name:'准备固定消息验证',exact:true}).click();
  await region.getByRole('button',{name:'审阅验证消息',exact:true}).click();
  const dialog=page.getByRole('dialog',{name:'确认发送验证消息',exact:true});await dialog.waitFor();
  const confirm=dialog.getByRole('button',{name:'确认发送一次',exact:true});assert.equal(await confirm.isDisabled(),true);
  const before=await request({type:'snapshot'});assert.equal(before.desktopAccess.trials.length,0);assert.ok(before.desktopAccess.trialReview.prompt.includes('不调用工具'));
  await dialog.getByRole('checkbox').check();await dialog.screenshot({path:join(report,'trial-review.png')});await confirm.click();await dialog.waitFor({state:'hidden'});
  await until(async()=>{const s=await request({type:'snapshot'});if(!s.desktopAccess.checking&&s.desktopAccess.error)throw Error(s.desktopAccess.error);return s.desktopAccess.trials.some(t=>t.proof);},'verified trial result');
  await region.getByRole('heading',{name:'文本往返已核实',exact:true}).waitFor({timeout:20000});
  const sent=await request({type:'snapshot'});assert.equal(sent.desktopAccess.trials.length,1);assert.ok(sent.desktopAccess.trials[0].proof);assert.equal(sent.desktopAccess.trialReview,null);
  await assert.rejects(request({type:'sendDesktopTrial',reviewDigest:before.desktopAccess.trialReview.id}),'duplicate approval must reject');
  assert.equal(sent.projectRuns.length,0);assert.equal(sent.apiJobs.length,0);await region.screenshot({path:join(report,'trial-passed.png')});
  await close();await until(()=>server.exitCode!==null,'fixture service exit');assert.equal(server.exitCode,0,serverLog);
  await open();const after=await request({type:'snapshot'});assert.deepEqual(after.desktopAccess.trials,sent.desktopAccess.trials);assert.equal(after.desktopAccess.trialReview,null);assert.equal(after.desktopAccess.checking,false);
  await page.getByRole('heading',{name:'文本往返已核实',exact:true}).waitFor();await page.getByRole('region',{name:'已有桌面会话',exact:true}).screenshot({path:join(report,'trial-restored.png')});assert.deepEqual(errors,[]);
  await close();await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',scope:live?'actual production Tauri/IPC/core/embedded bridge, authorized existing Qoder chat, one fixed text roundtrip and reopen':'actual Tauri/IPC confirmation and durable intent/proof with synthetic external responses; production-core reopen',confirmationRequired:true,duplicateSendRejected:true,restoredWithoutReplay:true,externalMessages:live?1:0,modelCalls:live?1:0,pageErrors:errors},null,2));console.log(JSON.stringify({report,status:'passed'}));
}catch(error){await page?.screenshot({path:join(report,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(report,'failure.json'),JSON.stringify({error:String(error),errors,serverLog,snapshot:await request({type:'snapshot'}).catch(()=>null)},null,2));throw error;}
finally{await writeFile(join(report,'service.log'),serverLog);await browser?.close().catch(()=>{});for(const child of [app,server])if(child&&child.exitCode===null){const exit=once(child,'exit');child.kill();await exit;}}
