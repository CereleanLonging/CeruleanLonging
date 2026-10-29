// Actual production component with synthetic snapshots; no native IPC or model.
import {readFile, writeFile, mkdtemp} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {build} from 'vite';
import {chromium} from 'playwright-core';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/desktop-task-reader-ui-'));
const entry=join(report,'entry.tsx');
await writeFile(entry,`import React from 'react';import {createRoot} from 'react-dom/client';import {DesktopTasks} from '../../apps/desktop/src/DesktopTasks';const root=createRoot(document.getElementById('root'));window.commands=[];window.renderJob=({job,reader='current-reader'})=>root.render(<DesktopTasks data={{desktopAccess:{tasks:[job],bindings:[],trials:[],checking:false,bridgeSha256:reader},workflows:[],projectRuns:[]}} busy={false} send={async c=>{window.commands.push(c);return true;}}/>);`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:join(report,'bundle'),emptyOutDir:false,lib:{entry,name:'ProgressUI',formats:['iife'],fileName:()=> 'ui.js'}}});
const review={id:'review',bindingId:'binding',projectId:'project',prompt:'owned test',attempt:{id:'attempt',taskId:'task',binding:{workspace:'D:\\fixture'}},observation:{capturedAt:new Date().toISOString(),modelLabel:'fixture',sessionId:null,window:{title:'Owned WorkBuddy',app:'workbuddy',version:'5.7.7.0'}}};
const latest={statusText:'生成中',observation:{busy:true,capturedAt:new Date().toISOString()}};
const job={review,active:true,submitted:false,retired:false,progress:{checking:false,samples:1,latest,error:null}};
let browser,networkRequests=0,result; const errors=[],cases=[];
try {
  browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
  const page=await browser.newPage();page.setDefaultTimeout(10000);
  page.on('pageerror',e=>errors.push(e.message));
  await page.route('**/*',route=>{networkRequests++;return route.abort();});
  await page.setContent('<html lang="zh-CN"><meta charset="utf-8"><div id="root"></div></html>');
  await page.addScriptTag({content:await readFile(join(report,'bundle/ui.js'),'utf8')});
  const render=async value=>{await page.evaluate(value=>window.renderJob(value),value);};
  const unknown={...job,active:false,review:{...review,bridgeSha256:'original-reader'}};
  const button=()=>page.getByRole('button',{name:'回读原任务结果',exact:true});
  await render({job:unknown});await page.getByText(/适配器已更新/).waitFor();
  await button().click();await page.waitForFunction(()=>window.commands.length===1);
  assert.deepEqual(await page.evaluate(()=>window.commands[0]),{type:'readDesktopTask',projectId:'project',attemptId:'attempt',readerSha256:'current-reader'});
  cases.push('upgraded reader is explicit; one read command and no send');
  await render({job:unknown,reader:'newer-reader'});
  await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
  await button().click();await page.waitForFunction(()=>window.commands.length===2);
  assert.equal(await page.evaluate(()=>window.commands[1].readerSha256),'newer-reader');
  cases.push('rerender uses latest reader identity');
  await render({job:{...unknown,reviewedStopRecord:'stop-review'}});
  await page.getByText(/原停止记录已复核/).waitFor();assert.equal(await button().isDisabled(),true);
  cases.push('reviewed stop remains read-blocked');
  await render({job:{...unknown,retired:true}});
  await page.getByRole('heading',{name:'本地跟踪已结束，结果未确认',exact:true}).waitFor();assert.equal(await button().count(),0);
  cases.push('retired original cannot be recovered');
  await render({job:{...unknown,submitted:true}});
  await page.getByRole('heading',{name:'交付已提交，等待验收',exact:true}).waitFor();assert.equal(await button().count(),0);
  cases.push('submitted original cannot be recovered again');
  assert.equal(await page.evaluate(()=>window.commands.length),2);assert.deepEqual(errors,[]);assert.equal(networkRequests,0);
  result={status:'passed',cases,syntheticReadCommands:2,desktopMessages:0,networkRequests,errors};
}catch(e){result={status:'failed',cases,error:e.stack};process.exitCode=1;}
finally{if(browser)await browser.close();}
await writeFile(join(report,'result.json'),JSON.stringify({...result,browserClosed:!browser?.isConnected()},null,2)+'\n');
console.log(JSON.stringify({report,...result}));
