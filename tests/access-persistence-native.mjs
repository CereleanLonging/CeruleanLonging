import {confirmConnectionContinue} from './connection-ui-helpers.mjs';
import {openConfiguredConnections,openConnectionMethod} from "./connection-ui-helpers.mjs";
// Real desktop restarts with fresh WebView caches and a synthetic workbench only.
import {chromium} from 'playwright-core';
import {spawn,spawnSync} from 'node:child_process';
import {createServer} from 'node:net';
import {once} from 'node:events';
import {mkdtemp,mkdir,writeFile,readdir,readFile,chmod} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
const run=await mkdtemp(resolve('reports/access-native-787-')),data=join(run,'data'),settings=join(run,'settings'),project=join(run,'project');
await mkdir(project);const cases=[],processes=[];let app,browser,page;
const wait=async fn=>{for(let i=0;i<160;i++){const value=await fn();if(value)return value;await new Promise(r=>setTimeout(r,150));}throw Error('Native condition timed out');};
const invoke=(name,args)=>page.evaluate(({name,args})=>window.__TAURI_INTERNALS__.invoke(name,args),{name,args});
async function launch(round){
  const server=createServer();server.listen(0,'127.0.0.1');await once(server,'listening');const port=server.address().port;await new Promise(r=>server.close(r));
  app=spawn(resolve('target/debug/cerulean-desktop.exe'),[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(run,'webviews-'+round),CERULEAN_TEST_MAINTENANCE_ROOT:join(run,'maintenance'),CERULEAN_TEST_ACCESS_ROOT:settings,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});processes.push(app.pid);
  browser=await wait(async()=>{try{return await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:600});}catch{return null;}});
  page=await wait(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')));page.setDefaultTimeout(20000);
  await page.getByText('本地核心已连接',{exact:true}).waitFor();
}
async function close(){
  await page.getByRole('button',{name:'退出工作台',exact:true}).click();await page.getByRole('button',{name:'确认退出并停止后台',exact:true}).click();await wait(()=>app.exitCode!==null);await browser.close().catch(()=>{});
}
try{
  await launch(1);
  await invoke('core_submit',{command:{type:'registerProject',project:{id:'fixture',root:project,timezone:'UTC'}}});
  await page.locator('.sidebar nav').getByRole('button',{name:'接入管理',exact:false}).click();await page.locator('#planning-project').waitFor();assert.equal(await page.locator('.connection-method[open]').count(),0);await openConnectionMethod(page,'主指挥','desktop');await openConfiguredConnections(page);await openConnectionMethod(page,'主指挥','desktop');await page.locator('#planning-chief-session').fill('native_session_fixture');await page.locator('#planning-deputy').fill('deputy-fixture');await page.locator('#planning-worker').fill('worker-fixture');assert.equal(await page.locator('#planning-executable').count(),0);
  await page.getByRole('button',{name:'下一步：设置需求与计划',exact:true}).click();await confirmConnectionContinue(page);await page.getByRole('button',{name:'修改接入配置',exact:true}).click();await page.getByText('格式检查通过，接入配置已自动保存；可随时修改。',{exact:true}).waitFor();
  const entries=await invoke('access_settings_load');assert(Object.values(entries).some(v=>v.includes('native_session_fixture')));
  let denied=false;try{await invoke('access_setting_put',{key:JSON.stringify(['cerulean-access-preference',1,'D:/unbound-workbench','chief-method']),value:'"desktop"'});}catch{denied=true;}assert(denied);
  cases.push('Real desktop auto-save is acknowledged by durable native storage; cross-workbench writes are rejected');
  await close();await launch(2);await page.locator('.sidebar nav').getByRole('button',{name:'接入管理',exact:false}).click();await page.locator('#planning-chief-session').waitFor({state:'attached'});assert.equal(await page.locator('.connection-method[open]').count(),0);assert.equal(await page.locator('#planning-chief-session').inputValue(),'native_session_fixture');assert.equal(await page.locator('#planning-worker').inputValue(),'worker-fixture');
  await page.getByText('已恢复自动保存的接入配置，格式检查已通过；可直接修改。',{exact:true}).waitFor();await page.screenshot({path:join(run,'native-restored.png'),fullPage:false});
  cases.push('A second native process restores session and model settings with all method panels collapsed');
  const storageFile=join(settings,(await readdir(settings))[0],'settings.json');await chmod(storageFile,0o444);
  await openConnectionMethod(page,'主指挥','desktop');await page.locator('#planning-chief-session').fill('native_custom_session');await page.getByRole('alert').filter({hasText:'接入设置尚未写入本机文件'}).first().waitFor();
  await page.getByRole('button',{name:'退出工作台',exact:true}).click();await page.getByRole('button',{name:'确认退出并停止后台',exact:true}).click();await page.getByRole('dialog').getByRole('alert').filter({hasText:'接入设置尚未保存'}).waitFor();assert.equal(app.exitCode,null);
  await page.getByRole('button',{name:'返回工作台',exact:true}).click();await chmod(storageFile,0o666);await page.getByRole('button',{name:'重试保存接入设置',exact:true}).click();await page.getByRole('alert').filter({hasText:'接入设置尚未写入本机文件'}).first().waitFor({state:'detached'});
  cases.push('Native disk-write failure is visible and normal exit waits; explicit retry persists the retained edit');
  await page.getByRole('button',{name:'下一步：设置需求与计划',exact:true}).click();await confirmConnectionContinue(page);await page.getByRole('button',{name:'修改接入配置',exact:true}).click();await page.getByText('格式检查通过，接入配置已自动保存；可随时修改。',{exact:true}).waitFor();await close();
  const folders=await readdir(settings);const saved=JSON.parse(await readFile(join(settings,folders[0],'settings.json'),'utf8'));assert(Object.values(saved.entries).some(v=>v.includes('native_custom_session')));
  await launch(3);await page.locator('.sidebar nav').getByRole('button',{name:'接入管理',exact:false}).click();await page.locator('#planning-chief-session').waitFor({state:'attached'});assert.equal(await page.locator('.connection-method[open]').count(),0);await chmod(storageFile,0o444);await openConnectionMethod(page,'主指挥','desktop');await page.locator('#planning-chief-session').fill('discard_this_unsaved_edit');await page.getByRole('alert').filter({hasText:'接入设置尚未写入本机文件'}).first().waitFor();
  await page.getByRole('button',{name:'退出工作台',exact:true}).click();await page.getByRole('button',{name:'确认退出并停止后台',exact:true}).click();await page.getByRole('button',{name:'放弃未保存的接入修改并退出',exact:true}).click();await wait(()=>app.exitCode!==null);await browser.close().catch(()=>{});await chmod(storageFile,0o666);
  assert.equal((await readFile(storageFile,'utf8')).includes('discard_this_unsaved_edit'),false);
  cases.push('Explicit discard exits normally after a write failure while preserving the previously saved selection');
  const healthy=await readFile(storageFile);await writeFile(storageFile,'broken fixture');await launch(4);await page.getByRole('alert').filter({hasText:'无法读取本机接入配置'}).waitFor();await close();assert.equal(await readFile(storageFile,'utf8'),'broken fixture');await writeFile(storageFile,healthy);
  cases.push('Unreadable native settings are preserved, show a load error and still allow normal exit without edits');
  const status=spawnSync(resolve('target/debug/cerulean-runtime.exe'),['--workbench','--data-dir',data,'status'],{windowsHide:true,encoding:'utf8',timeout:15000});
  await writeFile(join(run,'core-after-stop.json'),JSON.stringify({exitCode:status.status,stdout:status.stdout,stderr:status.stderr},null,2));
  cases.push('Custom modification replaces the stored selection; normal exit flushes writes and closes owned GUIs');
  await writeFile(join(run,'result.json'),JSON.stringify({status:'passed',cases,run,processes,allGuiProcessesExited:true,providerCalls:0,userDataOpened:false,forcedTermination:false},null,2));console.log(JSON.stringify({status:'passed',cases:cases.length,run}));
}catch(error){await page?.screenshot({path:join(run,'failure.png')}).catch(()=>{});await writeFile(join(run,'failure.json'),JSON.stringify({error:String(error),stack:error.stack,cases,processes},null,2));throw error;}
finally{
  if(app?.exitCode===null&&page&&!page.isClosed())await invoke('close_workbench').catch(()=>{});
  await browser?.close().catch(()=>{});
  if(app?.exitCode===null)await wait(()=>app.exitCode!==null).catch(()=>console.error('Owned GUI exit unconfirmed',app.pid));
  spawnSync(resolve('target/debug/cerulean-runtime.exe'),['--workbench','--data-dir',data,'service','stop'],{windowsHide:true,encoding:'utf8',timeout:20000});
}
