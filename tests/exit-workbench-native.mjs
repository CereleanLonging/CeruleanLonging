// Real WebView and native shutdown, using only a fresh synthetic workbench.
import { chromium } from 'playwright-core';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import assert from 'node:assert/strict';

const run=await mkdtemp(resolve('reports/exit-native-785-'));
const data=join(run,'data'),runtime=resolve('target/debug/cerulean-runtime.exe');
const server=createServer();server.listen(0,'127.0.0.1');await once(server,'listening');
const port=server.address().port;await new Promise(r=>server.close(r));
const app=spawn(resolve('target/debug/cerulean-desktop.exe'),[],{windowsHide:true,stdio:'ignore',env:{...process.env,
  CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(run,'webviews'),CERULEAN_TEST_MAINTENANCE_ROOT:join(run,'maintenance'),
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`,
}});
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
const wait=async fn=>{for(let i=0;i<100;i++){const result=await fn();if(result)return result;await sleep(150);}throw Error('Native condition timed out');};
const invoke=(page,name,args)=>page.evaluate(({name,args})=>window.__TAURI_INTERNALS__.invoke(name,args),{name,args});
const alive=pid=>{try{process.kill(pid,0);return true;}catch(e){if(e.code==='ESRCH')return false;throw e;}};
let browser,main,corePid;const cases=[];
try{
  browser=await wait(async()=>{try{return await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:700});}catch{return null;}});
  main=await wait(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')));
  main.setDefaultTimeout(15000);
  await main.getByText('尚未登记项目。',{exact:true}).waitFor();
  assert.equal(await main.getByRole('button',{name:'查看已保存项目配置',exact:true}).count(),0);
  const status=spawnSync(runtime,['--workbench','--data-dir',data,'status'],{windowsHide:true,encoding:'utf8',timeout:15000});
  assert.equal(status.status,0,status.stderr);corePid=JSON.parse(status.stdout).serverPid;assert(Number.isInteger(corePid));
  const label=await main.evaluate(()=>window.__TAURI_INTERNALS__.metadata.currentWindow.label);
  await invoke(main,'plugin:window|close',{label});
  await main.getByRole('dialog',{name:'确认退出工作台？'}).waitFor();
  assert.equal(await main.locator('dialog').evaluate(d=>d.matches(':modal')),true);
  const buttons=main.locator('.workspace-pagination button');assert.equal(await buttons.count(),2);
  for(const b of await buttons.all()){
    const box=await b.boundingBox();assert(box);
    assert.equal(await main.evaluate(({x,y,width,height})=>!!document.elementFromPoint(x+width/2,y+height/2)?.closest('dialog'),box),true);
  }
  for(const key of ['Tab','Tab','Shift+Tab']){await main.keyboard.press(key);assert.equal(await main.evaluate(()=>!!document.activeElement?.closest('main,aside')),false);}
  await main.screenshot({path:join(run,'native-exit-modal.png')});
  cases.push('native window close presents top-layer confirmation over both real pagination buttons with keyboard isolation');
  await main.keyboard.press('Escape');await main.getByRole('dialog').waitFor({state:'detached'});assert(alive(corePid));assert.equal(app.exitCode,null);
  await main.getByRole('button',{name:'维护与升级',exact:false}).click();await main.getByRole('button',{name:'打开只读日志窗口'}).waitFor();
  cases.push('Escape uses native cancellation, keeps the same core alive and restores page navigation');
  await main.getByRole('button',{name:'退出工作台',exact:true}).click();
  await main.getByRole('button',{name:'确认退出并停止后台',exact:true}).click();
  await wait(()=>app.exitCode!==null);await wait(()=>!alive(corePid));
  cases.push('confirmed exit normally stops the original synthetic core and closes its GUI');
  await writeFile(join(run,'result.json'),JSON.stringify({status:'passed',cases,run,data,appPid:app.pid,corePid,coreStopped:true,providerCalls:0,userDataOpened:false,forcedTermination:false},null,2));
  console.log(JSON.stringify({status:'passed',cases:cases.length,run}));
}catch(e){await main?.screenshot({path:join(run,'failure.png')}).catch(()=>{});await writeFile(join(run,'failure.json'),JSON.stringify({error:String(e),cases},null,2));throw e;}
finally{
  if(main&&!main.isClosed())await invoke(main,'close_workbench').catch(e=>console.error('Owned normal exit unconfirmed:',String(e)));
  await browser?.close().catch(()=>{});
  if(app.exitCode===null)await wait(()=>app.exitCode!==null).catch(()=>console.error('Owned GUI still active:',app.pid));
  if(corePid&&alive(corePid)){const r=spawnSync(runtime,['--workbench','--data-dir',data,'service','stop'],{windowsHide:true,encoding:'utf8',timeout:20000});if(r.status!==0)console.error('Synthetic core cleanup unconfirmed:',r.stderr);}
}
