import assert from 'node:assert/strict';
import {mkdtemp,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {build} from 'vite';
import {chromium} from 'playwright-core';
assert(process.env.CERULEAN_CACHE_SESSION);
const run=await mkdtemp(resolve('reports/activity-stream-ui-'));
const bundle=join(process.env.CERULEAN_CACHE_SESSION,'activity-stream-ui');
const entry=join(run,'entry.tsx');
await writeFile(entry,`import React from 'react';import{createRoot}from'react-dom/client';import{flushSync}from'react-dom';import{ProjectActivity}from'../../apps/desktop/src/ProjectActivity';
const root=createRoot(document.getElementById('root'));window.calls=[];window.count=90;window.failure=false;window.delay=false;
function record(n,project){return{anchor:{sequence:n,hash:project+'-hash-'+n},at:'2026-11-06T00:00:00Z',principal:n%2?'chief':'worker-deepseek',model:n%2?null:'deepseek-fixture',taskId:'t',attemptId:null,title:n===1?'主指挥提出方案':n===100?'执行模型提交结果':'工作事件 '+n,detail:project+' synthetic record '+n,files:n===1||n===100?['src/main.mjs']:[]};}
async function read(query){window.calls.push(query);if(window.delay&&query.projectId==='p'){await new Promise(r=>window.release=r);}if(window.failure)throw Error('原项目由独占写者持有，等待同步');const total=query.projectId==='p'?window.count:1;const start=query.after?query.after.sequence:query.before?Math.max(0,query.before.sequence-41):Math.max(0,total-40);const end=query.before?query.before.sequence-1:Math.min(total,start+40);return{type:'projectActivity',value:{projectId:query.projectId,entries:Array.from({length:end-start},(_,i)=>record(start+i+1,query.projectId)),head:total?record(total,query.projectId).anchor:null,hasOlder:start>0,hasNewer:end<total,total}};}
window.render=(data)=>flushSync(()=>root.render(<ProjectActivity data={data} read={read}/>));`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:bundle,emptyOutDir:false,lib:{entry,name:'ActivityFixture',formats:['iife'],fileName:()=> 'ui.js'}}});
const browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
const page=await browser.newPage({viewport:{width:1280,height:960}});page.setDefaultTimeout(10000);
const errors=[],cases=[];let requests=0;page.on('pageerror',e=>errors.push(e.message));
await page.route('**/*',r=>r.request().url()==='https://cerulean.test/'?r.fulfill({contentType:'text/html',body:'<html lang="zh-CN"><meta charset="utf-8"><div id="root"></div></html>'}):(requests++,r.abort()));
const data={workflows:[{project:{id:'p',root:'D:/synthetic/project'}},{project:{id:'q',root:'D:/synthetic/other'}}],projectSetups:[{setup:{projectId:'p',chiefModel:'MUST-NOT-INFER-HISTORICAL-MODEL'}}],managementDecisions:[{projectId:'p',operationId:'planning',purpose:'plan',state:'awaitingExternal',message:'等待主指挥会话'},{projectId:'p',operationId:'assign',purpose:'assign',state:'running'}],apiJobs:[{projectId:'p',attemptId:'api',taskId:'api-task',worker:{model:'api-fixture'},state:'executing'}],ompJobs:[{projectId:'p',attemptId:'omp',taskId:'omp-task',worker:{model:'omp-fixture'},state:'submitted'}],codexJobs:[],desktopAccess:{tasks:[{review:{projectId:'p',attempt:{id:'qoder',taskId:'desktop-task'},observation:{modelLabel:'qoder-fixture'}},active:true,progress:{latest:{statusText:'Qoder 正在工作'}}}]}};
try{
 await page.goto('https://cerulean.test/');await page.addStyleTag({path:'apps/desktop/src/style.css'});await page.addScriptTag({path:join(bundle,'ui.js')});await page.evaluate(data=>window.render(data),data);
 const log=page.getByRole('log',{name:'持续工作记录'}),entries=log.locator('.activity-entry');await page.waitForFunction(()=>document.querySelectorAll('.activity-entry').length===40);
 assert((await entries.first().innerText()).includes('#51'));assert.equal(await log.locator('summary').count(),0);assert.equal(await page.getByText('MUST-NOT-INFER-HISTORICAL-MODEL',{exact:false}).count(),0);
 for(const text of ['api-fixture','omp-fixture','qoder-fixture','等待主指挥会话'])assert((await page.locator('.activity-live').innerText()).includes(text));
 assert((await log.boundingBox()).height>=400);cases.push('Large stream starts at last committed page and shows real role/adapter states without guessing historical manager models');
 await page.evaluate(()=>window.count=91);await page.waitForFunction(()=>document.querySelectorAll('.activity-entry').length===41);await page.waitForTimeout(2200);assert.equal(await entries.count(),41);cases.push('Incremental polling appends and does not duplicate unchanged records');
 await page.getByRole('button',{name:'暂停记录刷新',exact:true}).click();await page.evaluate(()=>window.count=92);await page.waitForTimeout(2200);assert.equal(await entries.count(),41);
 await page.getByRole('button',{name:'恢复记录刷新',exact:true}).click();await page.waitForFunction(()=>document.querySelectorAll('.activity-entry').length===42);cases.push('Pause affects only log refresh; resuming reads from original anchor');
 await page.getByRole('button',{name:'读取更早记录',exact:true}).click();await page.waitForFunction(()=>document.querySelectorAll('.activity-entry').length===82);assert((await entries.first().innerText()).includes('#11'));
 await page.evaluate(()=>window.count=100);await page.waitForTimeout(2200);assert.equal(await entries.count(),82);assert((await page.getByRole('status').innerText()).includes('正在阅读历史'));
 await page.getByRole('button',{name:'跟随最新',exact:true}).click();await page.waitForFunction(()=>document.querySelectorAll('.activity-entry').length===40&&document.querySelector('.activity-entry').innerText.includes('#61'));assert.equal(await log.getByText('交付文件摘要',{exact:true}).count(),1);cases.push('Deliverable summaries appear only with proposals and submitted execution results');cases.push('Historical reading stays put and follow-latest reloads tail without gaps after window trimming');
 await page.getByLabel('筛选模型或角色').selectOption('deepseek-fixture');assert.equal(await entries.count(),20);await page.getByLabel('筛选模型或角色').selectOption('');
 await page.evaluate(()=>window.failure=true);await page.getByRole('button',{name:'重新读取记录',exact:true}).click();await page.getByRole('alert').waitFor();assert.equal(await entries.count(),40);await page.evaluate(()=>window.failure=false);cases.push('Filtering and read errors preserve committed content');
 await page.setViewportSize({width:420,height:900});assert(await page.locator('.project-activity').evaluate(e=>e.scrollWidth<=e.clientWidth+1));await page.screenshot({path:join(run,'stream-narrow.png'),fullPage:true});
 await page.setViewportSize({width:1280,height:960});await page.evaluate(()=>window.delay=true);await page.getByRole('button',{name:'重新读取记录',exact:true}).click();await page.waitForFunction(()=>!!window.release);
 await page.getByLabel('查看项目').selectOption('q');await page.waitForFunction(()=>document.querySelectorAll('.activity-entry').length===1);await page.evaluate(()=>window.release());await page.waitForTimeout(200);assert.equal(await entries.count(),1);assert((await log.innerText()).includes('q synthetic record'));cases.push('Project switch discards delayed old-project response; narrow layout has no horizontal overflow');
 await page.screenshot({path:join(run,'stream-wide.png'),fullPage:true});assert.deepEqual(errors,[]);assert.equal(requests,0);
 await writeFile(join(run,'result.json'),JSON.stringify({passed:true,cases,providerCalls:0,scope:'Real React/CSS with synthetic IPC records; no native model tasks'},null,2));console.log(JSON.stringify({passed:true,cases:cases.length,report:run}));
}catch(error){await page.screenshot({path:join(run,'failure.png'),fullPage:true});await writeFile(join(run,'failure.json'),JSON.stringify({error:String(error),errors,cases},null,2));throw error;}finally{await browser.close();}
