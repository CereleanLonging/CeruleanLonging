// Real WebView windows and IPC, using only a fresh synthetic workbench.
import { chromium } from 'playwright-core';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import assert from 'node:assert/strict';

const run = await mkdtemp(resolve('reports/review-logs-native-784-'));
const data = join(run, 'data');
const runtime = resolve('target/debug/cerulean-runtime.exe');
const server = createServer();server.listen(0, '127.0.0.1');await once(server, 'listening');
const port = server.address().port;await new Promise(r => server.close(r));
const app = spawn(resolve('target/debug/cerulean-desktop.exe'), [], {
  windowsHide: true, stdio: 'ignore', env: { ...process.env,
    CERULEAN_TEST_DATA_DIR: data, CERULEAN_TEST_WEBVIEW_ROOT: join(run, 'webviews'),
    CERULEAN_TEST_MAINTENANCE_ROOT: join(run, 'maintenance'),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`,
  },
});
const sleep = ms => new Promise(r => setTimeout(r, ms));
const wait = async fn => { for(let i=0;i<100;i++){const result=await fn();if(result)return result;await sleep(150);}throw Error('Native condition timed out'); };
const cases = [];let browser, main, originalCorePid;
const alive = pid => {try {process.kill(pid,0);return true;}catch(error){if(error.code==='ESRCH')return false;throw error;}};
const invoke = (page, name, args) => page.evaluate(({name,args}) => window.__TAURI_INTERNALS__.invoke(name,args), {name,args});
const status = () => {
  const result = spawnSync(runtime, ['--workbench','--data-dir',data,'status'], {windowsHide:true,encoding:'utf8',timeout:15000});
  assert.equal(result.status,0,result.stderr);return JSON.parse(result.stdout);
};
try {
  browser = await wait(async()=>{try{return await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:700});}catch{return null;}});
  main = await wait(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')));
  main.setDefaultTimeout(15000);
  await main.getByText('尚未登记项目。',{exact:true}).waitFor();
  assert.equal(await main.getByRole('button',{name:/查看全局.*复核记录/}).count(),0);
  const original = status();
  originalCorePid=original.serverPid;assert(Number.isInteger(originalCorePid));
  await main.getByRole('button',{name:'维护与升级',exact:false}).click();
  await main.getByRole('button',{name:'打开只读日志窗口'}).click();
  let logs = await wait(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().endsWith('#review-logs')));
  await logs.getByText('当前没有已复核的 API 中断记录。',{exact:true}).waitFor();
  await main.getByRole('button',{name:'独立实验工作台',exact:false}).click();
  const toggle=main.getByRole('checkbox',{name:'开启独立实验功能'});
  assert.equal(await toggle.isChecked(),false);
  assert.equal(await main.getByRole('region',{name:'独立实验工作台管理',exact:true}).count(),0);
  await main.screenshot({path:join(run,'native-experiments-default-off.png')});
  await toggle.check();
  await main.getByRole('region',{name:'独立实验工作台管理',exact:true}).waitFor();
  await main.getByRole('heading',{name:'团队效率验收',exact:true}).waitFor();
  await main.getByRole('heading',{name:'单执行者效率验收',exact:true}).waitFor();
  await main.screenshot({path:join(run,'native-experiments-enabled.png'),fullPage:true});
  await toggle.uncheck();
  assert.equal(await main.getByRole('heading',{name:'单执行者效率验收',exact:true}).count(),0);
  assert.equal((await invoke(main,'workspace_read',{query:{type:'overview'}})).value.projectCount,0);
  cases.push('native opt-in experiment page expands all tools and hides them without creating projects');
  await logs.getByRole('tab',{name:'管理复核记录',exact:true}).click();
  await logs.getByText('当前没有已复核的 管理残片记录。',{exact:true}).waitFor();
  cases.push('native independent window renders both read-only log categories');
  assert.equal(await logs.locator('input,textarea,form').count(),0);
  await assert.rejects(invoke(logs,'core_submit',{command:{type:'createPlan',title:'DENIED',tasks:['DENIED']}}),/窗口未绑定工作台/);
  await assert.rejects(invoke(logs,'core_request',{command:{type:'snapshot'}}),/窗口未绑定工作台/);
  await assert.rejects(invoke(logs,'workspace_read',{query:{type:'overview'}}),/窗口未绑定工作台/);
  await assert.rejects(invoke(logs,'review_logs_read',{query:{type:'overview'}}),/仅可读取/);
  await assert.rejects(invoke(logs,'open_maintenance_window'),/窗口未绑定工作台/);
  await assert.rejects(invoke(main,'review_logs_read',{query:{type:'apiPublicationReviews',offset:0,revision:null}}),/只读日志窗口/);
  cases.push('native mutation, general read, maintenance and wrong-window routes are denied');
  await main.getByRole('button',{name:'维护与升级',exact:false}).click();
  await main.getByRole('button',{name:'打开只读日志窗口'}).click();
  assert.equal(browser.contexts().flatMap(c=>c.pages()).filter(p=>p.url().endsWith('#review-logs')).length,1);
  assert.equal(status().serverPid,original.serverPid);
  await logs.screenshot({path:join(run,'native-readonly-logs.png')});
  // Use the real OS window-close request, not page.close / forced process termination.
  const label = await logs.evaluate(()=>window.__TAURI_INTERNALS__.metadata.currentWindow.label);
  await invoke(logs,'plugin:window|close',{label}).catch(e=>{if(!logs.isClosed())throw e;});
  await wait(()=>logs.isClosed());assert.equal(status().serverPid,original.serverPid);
  cases.push('opening reuses one log window; its normal close leaves the owner core running');
  await invoke(main,'open_review_logs_window');
  logs = await wait(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().endsWith('#review-logs')));
  await logs.getByText('当前没有已复核的 API 中断记录。',{exact:true}).waitFor();
  // Explicit owner shutdown must destroy its read-only child and stop the original core.
  await invoke(main,'close_workbench').catch(e=>{if(!main.isClosed())throw e;});
  await wait(()=>app.exitCode!==null);
  assert.equal(logs.isClosed(),true);
  await wait(()=>!alive(originalCorePid));
  cases.push('owner exit closes its logs and leaves no GUI process');
  await writeFile(join(run,'result.json'),JSON.stringify({status:'passed',cases,run,data,appPid:app.pid,corePid:original.serverPid,coreStopped:true,providerCalls:0,userDataOpened:false,forcedTermination:false},null,2));
  console.log(JSON.stringify({status:'passed',cases:cases.length,run}));
} catch(error) {
  await main?.screenshot({path:join(run,'failure.png')}).catch(()=>{});
  await writeFile(join(run,'failure.json'),JSON.stringify({error:String(error),stack:error.stack,cases},null,2));throw error;
} finally {
  if(main&&!main.isClosed())await invoke(main,'close_workbench').catch(e=>console.error('Owned normal exit unconfirmed:',String(e)));
  await browser?.close().catch(()=>{});
  // Only the synthetic directory is targeted; never force-kill user windows.
  if(app.exitCode===null)await wait(()=>app.exitCode!==null).catch(()=>console.error('Owned GUI still active:',app.pid));
  if(originalCorePid&&alive(originalCorePid)){
    const stopped=spawnSync(runtime,['--workbench','--data-dir',data,'service','stop'],{windowsHide:true,encoding:'utf8',timeout:20000});
    if(stopped.status!==0)console.error('Synthetic core cleanup unconfirmed:',stopped.stderr);
  }
}
