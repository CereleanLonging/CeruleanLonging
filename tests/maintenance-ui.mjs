import {readFile,writeFile,mkdtemp} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
import {build} from 'vite';
import {chromium} from 'playwright-core';
const run=await mkdtemp(resolve('reports/maintenance-ui-'));
const entry=join(run,'entry.tsx');
await writeFile(entry,`import React from 'react';import{createRoot}from'react-dom/client';import{Maintenance}from'../../apps/desktop/src/Maintenance';const root=createRoot(document.getElementById('root')!);window.renderFixture=(desktop,key)=>root.render(<Maintenance key={key} desktop={desktop}/>);window.renderFixture(false,0);`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:join(run,'bundle'),emptyOutDir:false,lib:{entry,name:'Maintenance',formats:['iife'],fileName:()=> 'form.js'}}});
const browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
const cases=[];const errors=[];
try{
  const page=await browser.newPage({viewport:{width:1200,height:900}});page.on('pageerror',e=>errors.push(e.message));await page.route('**/*',r=>r.abort());
  await page.setContent(`<html lang="zh-CN"><meta charset="utf-8"><style>${await readFile('apps/desktop/src/style.css','utf8')}</style><div id="root"></div></html>`);
  await page.evaluate(()=>{
    window.calls=[];window.pending=null;window.fail=false;
    window.__TAURI_INTERNALS__={invoke:async(name,args)=>{
      window.calls.push({name,args});
      if(name==='maintenance_info')return {version:'0.1.3',installationDirectory:'D:\\test install',dataDirectory:'D:\\test data',holdPresent:true,bundleSchema:1};
      if(name==='open_maintenance_window')return new Promise((ok,no)=>{window.pending={ok,no};});
      throw Error('Unexpected command '+name);
    }};
  });
  await page.addScriptTag({content:await readFile(join(run,'bundle/form.js'),'utf8')});
  const open=page.getByRole('button',{name:'打开独立维护窗口'});
  assert.equal(await open.isDisabled(),true);assert.equal(await page.evaluate(()=>window.calls.length),0);cases.push('preview-has-no-native-actions');
  await page.evaluate(()=>window.renderFixture(true,1));await page.waitForFunction(()=>window.calls.length===1);
  await page.getByText('已锁定，请读取原事务后恢复').waitFor();cases.push('held-data-does-not-require-core-connection');
  await open.click();assert.equal(await page.getByRole('button',{name:'正在准备维护副本…'}).isDisabled(),true);cases.push('launch-single-in-flight');
  await page.evaluate(()=>window.pending.ok({directory:'D:\\maintenance copy',sessionDigest:'a'.repeat(64),pid:1234,started:true}));
  await page.getByRole('status').waitFor();assert.match(await page.getByRole('status').innerText(),/不会停止核心或执行升级/);cases.push('launch-result-does-not-claim-upgrade');
  await open.click();await page.evaluate(()=>window.pending.no(Error('启动未确认，未重试')));await page.getByRole('alert').waitFor();
  assert.equal(await page.evaluate(()=>window.calls.filter(x=>x.name==='open_maintenance_window').length),2);cases.push('failure-not-retried');
  assert.equal(await page.evaluate(()=>window.calls.some(x=>x.name==='core_request')),false);cases.push('maintenance-entry-never-starts-core');
  await page.screenshot({path:join(run,'maintenance-entry.png')});assert.deepEqual(errors,[]);
  const result={status:'passed',run,passed:cases.length,cases,mockedTauri:true,realModelCalls:0,userDesktopActions:0};await writeFile(join(run,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
}finally{await browser.close();}
