import assert from 'node:assert/strict';
import {mkdtemp,writeFile,readFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {build} from 'vite';
import {chromium} from 'playwright-core';
assert(process.env.CERULEAN_CACHE_SESSION);
const run=await mkdtemp(resolve('reports/planning-progress-ui-'));
const bundle=join(process.env.CERULEAN_CACHE_SESSION,'planning-progress-ui');
const entry=join(run,'entry.tsx');
await writeFile(entry,`import React,{useState}from'react';import{createRoot}from'react-dom/client';import{flushSync}from'react-dom';
import{ProjectPlanning}from'../../apps/desktop/src/ProjectPlanning';import{newApiWorker}from'../../apps/desktop/src/ApiWorkerFields';import{defaultSupervision}from'../../apps/desktop/src/SupervisionFields';import{submitAndRead}from'../../apps/desktop/src/commandSubmission';
const root=createRoot(document.getElementById('root'));let epoch=0;
const setup={projectId:'project',version:1,requirement:'合成需求',chiefDirectory:'D:/synthetic/project-chief',managementSources:{chief:{kind:'codexSession',sessionId:'synthetic-chief'},deputy:{kind:'openAi'}},workerAdapter:'deepseek',apiWorker:{...newApiWorker('deepseek'),model:'worker'},chiefModel:'external-codex-session',deputyModel:'deputy',validatorId:'cerulean-objective-checker',check:{args:[],timeoutSeconds:60},artifactPaths:[],managementMaxRequests:10,policy:{maxRequests:3,maxSeconds:180,supervision:{...defaultSupervision,maxAdvicePerAttempt:1,deliveryEnabled:true}}};
const base={dataDirectory:'synthetic-progress',workflows:[{project:{id:'project',root:'D:/synthetic/project'},state:{tasks:{}},approval:null,revision:null}],projectSetups:[{digest:'a'.repeat(64),setup}],workerAccountSetups:[],validators:[{id:'cerulean-objective-checker',name:'Fixture checker'}],managementDecisions:[],managementHistory:{projects:[]},openaiConnection:{models:[],keyConfigured:false},codexConnection:{models:[],executable:''},providerCredentials:[],providerConnections:[],projectRuns:[],codexJobs:[],apiJobs:[],adapters:[],desktopBindings:[]};
function Test(){const[data,setData]=useState(structuredClone(base)),[busy,setBusy]=useState(false);window.replace=value=>setData(structuredClone(value));
 const send=async(command,onError)=>{setBusy(true);const outcome=await submitAndRead(command,async c=>{window.commands.push(c);if(c.type==='snapshot')return{type:'snapshot',value:structuredClone(window.next)};if(window.hold)await new Promise(resolve=>window.release=resolve);if(window.failure)throw Error(window.failure);return{type:'receipt',value:{requestId:'fixture'}};},async()=>{if(window.failRefresh)throw Error('合成刷新失败');return structuredClone(window.next);});
 if(outcome.type==='unconfirmed'){onError?.(outcome.error);setBusy(false);return false;}if(outcome.snapshot)setData(outcome.snapshot);if(outcome.type==='accepted'&&outcome.refreshError)onError?.('操作确认已收到，完整视图刷新失败；请读取原记录核对。'+outcome.refreshError);setBusy(false);return true;};
 return <ProjectPlanning data={data} mode="planning" busy={busy} send={send}/>;}
window.show=()=>{epoch++;window.commands=[];window.next=structuredClone(base);window.hold=false;window.failure='';window.failRefresh=false;flushSync(()=>root.render(<Test key={epoch}/>));};
window.record=(state,extra={})=>{window.next.managementDecisions=[{operationId:'ui-plan-synthetic-1',projectId:'project',purpose:'plan',state,message:null,hasWait:false,hasReply:false,...extra}];window.next.managementHistory.projects=[{projectId:'project',total:1,pending:['running','awaitingExternal','ready','applying','unknown'].includes(state)?1:0,running:state==='running'?1:0}];window.replace(window.next);};
`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:bundle,emptyOutDir:false,lib:{entry,name:'PlanningProgress',formats:['iife'],fileName:()=> 'ui.js'}}});
const browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
const page=await browser.newPage({viewport:{width:1200,height:1000}}),cases=[],errors=[];
page.setDefaultTimeout(12000);page.on('pageerror',e=>errors.push(e.message));
try{
 const css=await readFile('apps/desktop/src/style.css','utf8');
 await page.route('**/*',r=>r.request().url()==='https://cerulean.test/'?r.fulfill({contentType:'text/html',body:'<html lang="zh-CN"><meta charset="utf-8"><style>'+css+'body{padding:24px}</style><div id="root"></div></html>'}):r.abort());
 await page.goto('https://cerulean.test/');await page.addScriptTag({path:join(bundle,'ui.js')});await page.evaluate(()=>window.show());
 const panel=page.getByRole('region',{name:'主指挥规划请求',exact:true}),request=()=>panel.getByRole('button',{name:'请求主指挥生成方案',exact:true});
 await request().waitFor();assert(await request().isEnabled());
 await page.evaluate(()=>{window.hold=true;window.failure='主指挥材料共 164000 字节，超过本次 64 KiB 容量，请拆分需求后保存配置';});
 await request().dblclick();await panel.getByText('正在提交规划请求，等待核心确认…',{exact:true}).waitFor();
 assert.equal(await panel.getByRole('progressbar').getAttribute('value'),null);
 assert.equal(await page.evaluate(()=>window.commands.length),1);
 await page.evaluate(()=>window.release());await panel.getByRole('alert').filter({hasText:'164000'}).waitFor();assert(await request().isDisabled());
 await panel.getByRole('button',{name:'读取原请求记录',exact:true}).click();assert(await request().isEnabled());
 assert.deepEqual(await page.evaluate(()=>window.commands.map(c=>c.type)),['requestChiefPlan','snapshot']);
 cases.push('Immediate indeterminate progress, one mutation on double-click, inline original error and read-only reconciliation');
 await panel.screenshot({path:join(run,'inline-error.png')});
 await page.evaluate(()=>{window.show();window.failRefresh=true;});await request().click();await panel.getByRole('alert').filter({hasText:'操作确认已收到'}).waitFor();assert(await request().isDisabled());
 await page.evaluate(()=>{window.failRefresh=false;window.next.managementDecisions=[{operationId:'ui-plan-synthetic-1',projectId:'project',purpose:'plan',state:'awaitingExternal',message:null}];window.next.managementHistory.projects=[{projectId:'project',total:1,pending:1}];});
 await panel.getByRole('button',{name:'读取原请求记录',exact:true}).click();await panel.getByText('请求已保存，等待绑定的 Codex 聊天读取材料并回填方案。',{exact:true}).waitFor();assert.equal(await panel.getByRole('progressbar').getAttribute('value'),'1');
 assert.equal(await page.evaluate(()=>window.commands.filter(c=>c.type==='requestChiefPlan').length),1);
 cases.push('Accepted request with failed snapshot stays protected from resend and recovers original external-session status');
 for(const state of ['running','ready','applying']){await page.evaluate(state=>window.record(state),state);assert.equal(await panel.getByRole('progressbar').getAttribute('value'),state==='running'?'1':'2');}
 await page.evaluate(()=>window.record('applied',{hasWait:true,message:'请补充原始数据格式'}));assert.equal(await panel.getByRole('progressbar').getAttribute('value'),'2');await panel.getByRole('alert').filter({hasText:'请补充'}).waitFor();
 await page.evaluate(()=>{window.next.workflows[0].revision={managementSetupDigest:'a'.repeat(64)};window.record('applied');});await panel.getByText('方案已生成，请审阅方案后决定是否批准。',{exact:true}).waitFor();assert.equal(await panel.getByRole('progressbar').getAttribute('value'),'3');
 cases.push('Durable running, validating, waiting and applied states drive stages; a Wait reply never shows completed');
 for(const width of [1200,640,430]){await page.setViewportSize({width,height:1000});assert(await panel.evaluate(e=>e.scrollWidth<=e.clientWidth+1));await panel.screenshot({path:join(run,'progress-'+width+'.png')});}
 cases.push('Request, count, stages and readback actions fit wide and narrow layouts');
 await page.evaluate(()=>{window.show();window.hold=true;});await request().click();await page.waitForFunction(()=>!!window.release);
 await page.evaluate(()=>{const release=window.release;window.show();release();});await page.waitForTimeout(100);assert.equal(await panel.getByRole('alert').count(),0);assert(await request().isEnabled());
 cases.push('Late completion after a project component is replaced does not change the new request view');
 assert.deepEqual(errors,[]);await writeFile(join(run,'result.json'),JSON.stringify({passed:true,cases,errors,scope:'Production planning components and submission protocol with synthetic IPC; no model calls'},null,2));console.log(JSON.stringify({passed:true,cases:cases.length,report:run}));
}catch(error){await writeFile(join(run,'failure.json'),JSON.stringify({error:String(error.stack),cases,errors},null,2));await page.screenshot({path:join(run,'failure.png'),fullPage:true});throw error;}
finally{await browser.close();}
