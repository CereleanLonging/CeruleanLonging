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
const report=await mkdtemp(resolve('reports/desktop-trial-recovery-'));
const original=resolve(process.env.CERULEAN_NATIVE_TRIAL_RECOVERY??'');
assert.ok(original.startsWith(resolve('reports')+'\\desktop-trial-native-'),'existing isolated trial report required');
const previous=JSON.parse(await readFile(join(original,'failure.json'),'utf8'));const expected=previous.snapshot.desktopAccess.trials[0];
assert.equal(expected.review.observation.sessionId,'76edb3bc-078f-4ce4-bc9b-b6525fde9c51');assert.equal(expected.proof,null);
const intent=join(original,'data','desktop-trials-v1',expected.review.id+'.intent.json');const intentBefore=await readFile(intent);
const sleep=ms=>new Promise(r=>setTimeout(r,ms));async function until(fn,label){for(let i=0;i<900;i++){const v=await fn();if(v)return v;await sleep(100);}throw Error('Timed out: '+label);}
let app,browser,page,data=join(original,'data');const errors=[];const request=command=>page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command);
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
  await open();const before=await request({type:'snapshot'});assert.equal(before.desktopAccess.trials.length,1);assert.deepEqual(before.desktopAccess.trials[0],expected);
  const region=page.getByRole('region',{name:'已有桌面会话',exact:true});await region.getByRole('button',{name:'只读核对结果',exact:true}).click();
  await until(async()=>{const s=await request({type:'snapshot'});if(!s.desktopAccess.checking&&s.desktopAccess.error)throw Error(s.desktopAccess.error);return s.desktopAccess.trials[0].proof;},'read-only recovery proof');
  await region.getByRole('heading',{name:'文本往返已核实',exact:true}).waitFor({timeout:20000});const after=await request({type:'snapshot'});assert.ok(after.desktopAccess.trials[0].proof);assert.equal(after.desktopAccess.trials[0].proof.readerSha256,after.desktopAccess.bridgeSha256);assert.ok(after.desktopAccess.trials[0].proof.observedBaseline.length>=2);assert.deepEqual(await readFile(intent),intentBefore,'original intent is immutable');
  assert.equal(after.projectRuns.length,0);await region.screenshot({path:join(report,'recovered.png')});await close();
  await open();const reopened=await request({type:'snapshot'});assert.deepEqual(reopened.desktopAccess.trials,after.desktopAccess.trials);assert.equal(reopened.desktopAccess.checking,false);await page.getByRole('heading',{name:'文本往返已核实',exact:true}).waitFor();await close();
  await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',scope:'actual production Tauri/IPC/core/embedded bridge read-only recovery of original live Qoder trial, then reopen',originalReport:original,originalTrialId:expected.review.id,proof:after.desktopAccess.trials[0].proof,originalIntentUnchanged:true,newMessages:0,newModelCalls:0,originalLiveRoundTripsVerified:1,pageErrors:errors},null,2));console.log(JSON.stringify({report,status:'passed'}));
}catch(error){await page?.screenshot({path:join(report,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(report,'failure.json'),JSON.stringify({error:String(error),errors,snapshot:await request({type:'snapshot'}).catch(()=>null)},null,2));throw error;}
finally{await browser?.close().catch(()=>{});if(app&&app.exitCode===null){const exit=once(app,'exit');app.kill();await exit;}}
