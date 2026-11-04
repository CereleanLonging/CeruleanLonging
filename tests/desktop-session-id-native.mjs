import assert from 'node:assert/strict';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve,toNamespacedPath} from 'node:path';
import {chromium} from 'playwright-core';
import {openConnectionMethod} from './connection-ui-helpers.mjs';

assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const run=await mkdtemp(resolve('reports/desktop-session-id-native-'));
const compiled=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{windowsHide:true,encoding:'utf8',timeout:180000});
assert.equal(compiled.status,0,compiled.stderr);
const testExe=compiled.stdout.split(/\r?\n/).filter(Boolean).map(line=>JSON.parse(line))
  .find(item=>item.reason==='compiler-artifact'&&item.profile.test&&item.executable)?.executable;
assert(testExe);
const regression=spawnSync(testExe,['session_id_','--nocapture'],{windowsHide:true,encoding:'utf8',timeout:60000});
assert.equal(regression.status,0,regression.stdout+regression.stderr);
const service=spawn(testExe,['--exact','host::desktop_sessions::tests::native_desktop_binding_server','--ignored','--nocapture'],{
  windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,CERULEAN_DESKTOP_NATIVE_DIR:run},
});
let serviceLog='';service.stdout.on('data',data=>serviceLog+=data);service.stderr.on('data',data=>serviceLog+=data);
const wait=async predicate=>{for(let i=0;i<200;i++){const value=await predicate();if(value)return value;await new Promise(r=>setTimeout(r,150));}throw Error('Session binding state timed out');};
const fixture=await wait(async()=>{assert.equal(service.exitCode,null,serviceLog);try{return JSON.parse(await readFile(join(run,'fixture.json'),'utf8'));}catch{return null;}});
const sessionId='12345678-1234-1234-1234-123456789abc';
const cases=[];
let app,browser,page;
const invoke=(name,args)=>page.evaluate(({name,args})=>window.__TAURI_INTERNALS__.invoke(name,args),{name,args});
const snapshot=()=>invoke('core_request',{command:{type:'snapshot'}});
async function open(round){
  const socket=createServer();socket.listen(0,'127.0.0.1');await once(socket,'listening');const port=socket.address().port;await new Promise(r=>socket.close(r));
  app=spawn(process.env.CERULEAN_NATIVE_GUI,[],{windowsHide:true,stdio:'ignore',env:{...process.env,
    CERULEAN_TEST_DATA_DIR:fixture.data,CERULEAN_TEST_ACCESS_ROOT:join(run,'settings'),
    CERULEAN_TEST_WEBVIEW_ROOT:join(run,'webview-'+round),CERULEAN_TEST_MAINTENANCE_ROOT:join(run,'maintenance'),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`,
  }});
  browser=await wait(async()=>{assert.equal(app.exitCode,null);try{return await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});}catch{return null;}});
  page=await wait(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')));
  page.setDefaultTimeout(15000);
  await page.getByText('本地核心已连接',{exact:true}).waitFor();
  assert.equal(toNamespacedPath(resolve((await snapshot()).dataDirectory)),toNamespacedPath(resolve(fixture.data)));
  await page.locator('.sidebar nav').getByRole('button',{name:'接入管理',exact:false}).click();
  await page.locator('.project-connections').waitFor();
  await openConnectionMethod(page,'执行端','desktop');
}
async function close(){
  await page.getByRole('button',{name:'退出工作台',exact:true}).click();
  await page.getByRole('button',{name:'确认退出并停止后台',exact:true}).click();
  await wait(()=>app.exitCode!==null);
  await browser.close();browser=null;
}
try{
  await open(1);
  const worker=page.getByRole('region',{name:'执行端',exact:true});
  await worker.getByRole('button',{name:'将桌面端接入设为默认执行端',exact:true}).click();
  const fields=worker.getByRole('region',{name:'桌面编程会话配置',exact:true}).first();
  const input=fields.getByLabel('绑定的 Qoder 会话 ID',{exact:true});
  const bind=fields.getByRole('button',{name:'测试并绑定会话',exact:true});
  await input.fill('partial-id');assert(await bind.isDisabled());
  await input.fill(sessionId);assert.equal(await input.evaluate(e=>e.checkValidity()),false);
  await bind.click();
  await fields.getByRole('status').filter({hasText:'会话已核对并绑定'}).waitFor();
  await fields.getByText('已绑定模型 · fixture-model',{exact:true}).waitFor();
  const stored=(await snapshot()).desktopAccess.bindings;
  assert.equal(stored.length,1);assert.equal(stored[0].observation.sessionId,sessionId);
  assert.equal(stored[0].projectId,'project');assert.equal(await input.evaluate(e=>e.checkValidity()),true);
  assert.equal(await fields.getByRole('button',{name:'发现已打开窗口',exact:true}).count(),0);
  cases.push('The actual desktop binds the entered UUID through authenticated IPC with synthetic read-only observations and fills the model');
  await fields.getByRole('checkbox').check();
  await bind.click();await fields.getByRole('status').filter({hasText:'会话已核对并绑定'}).waitFor();
  await wait(async()=>!(await bind.isDisabled()));
  assert.deepEqual((await snapshot()).desktopAccess.bindings,stored);
  assert(await fields.getByRole('checkbox').isChecked());
  cases.push('Repeated binding rechecks the same conversation and reuses its record and existing explicit scope confirmation');
  await input.fill('12345678-1234-1234-1234-123456789def');await bind.click();
  await fields.getByRole('alert').filter({hasText:'未找到所填会话 ID'}).waitFor();
  assert.equal(await fields.getByText('已绑定模型 · fixture-model',{exact:true}).count(),0);
  assert.equal(await input.evaluate(e=>e.checkValidity()),false);
  assert.deepEqual((await snapshot()).desktopAccess.bindings,stored);
  await input.fill(sessionId);await bind.click();await fields.getByRole('status').filter({hasText:'会话已核对并绑定'}).waitFor();
  cases.push('A mismatched ID clears the selected draft and shows a local error while retaining the original saved record');
  await fields.screenshot({path:join(run,'session-id-binding.png')});
  await worker.getByRole('button',{name:'添加桌面端接入执行配置',exact:true}).click();
  const profile=worker.getByRole('region',{name:'执行配置 1',exact:true});
  await profile.getByLabel('绑定的 Qoder 会话 ID',{exact:true}).fill(sessionId);
  await profile.getByRole('button',{name:'测试并绑定会话',exact:true}).click();
  await profile.getByRole('status').filter({hasText:'会话已核对并绑定'}).waitFor();
  assert.equal(await input.inputValue(),sessionId);
  assert.deepEqual((await snapshot()).desktopAccess.bindings,stored);
  cases.push('Additional desktop profiles use their own session field and resolve to verified project bindings');
  await wait(async()=>JSON.stringify(await invoke('access_settings_load')).includes(stored[0].id));
  const state=await snapshot();assert.equal(state.desktopAccess.tasks.length,0);assert.equal(state.desktopAccess.trials.length,0);
  await close();await wait(()=>service.exitCode!==null);assert.equal(service.exitCode,0,serviceLog);
  await open(2);
  assert.deepEqual((await snapshot()).desktopAccess.bindings,stored);
  assert.equal(await page.locator('#planning-desktop-session-id').inputValue(),sessionId);
  assert.equal(await page.locator('#profile-0-desktop-session-id').inputValue(),sessionId);
  cases.push('A fresh production core and WebView restore existing bindings and both profile session IDs without new observation or dispatch');
  await close();
  const result={status:'passed',cases,run,modelCalls:0,externalMessages:0,observations:'synthetic fixture through real native IPC',productionReopen:true};
  await writeFile(join(run,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
}catch(error){
  await page?.screenshot({path:join(run,'failure.png')}).catch(()=>{});
  await writeFile(join(run,'failure.json'),JSON.stringify({error:String(error),stack:error.stack,cases,serviceLog},null,2));throw error;
}finally{
  if(app?.exitCode===null&&page&&!page.isClosed())await invoke('close_workbench').catch(()=>{});
  await browser?.close().catch(()=>{});
  if(service.exitCode===null)spawnSync(process.env.CERULEAN_NATIVE_RUNTIME,['--stop','--data-dir',fixture.data],{windowsHide:true,encoding:'utf8',timeout:15000});
}
