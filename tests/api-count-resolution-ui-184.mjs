import {readFile,writeFile,mkdtemp} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
import {build} from 'vite';
import {chromium} from 'playwright-core';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const dir=await mkdtemp(resolve('reports/api-count-resolution-ui-184-'));
const cases=[],errors=[];let browser,page,networkRequests=0;
const entry=join(dir,'entry.tsx');
await writeFile(entry,`import React from 'react';import {createRoot} from 'react-dom/client';import {ApiCountPanel} from '../../apps/desktop/src/ApiCountPanel';
const root=createRoot(document.getElementById('root')!);let serial=0;window.allCommands=[];
const item={projectId:'project',requestId:'api-'+ 'a'.repeat(64),taskId:'task',attemptId:'original-attempt',round:2,originalOutcome:'unknown',maximumUsdNanos:1000000,feeResolution:null};
async function query(c){window.commands.push(c);window.allCommands.push({mode:window.mode,command:c});
 if(c.type==='readApiCounts')return {apiCounts:{revision:window.mode==='stalePage'&&c.offset?'changed':'a'.repeat(64),offset:c.offset,total:window.mode==='stalePage'?2:1,nextOffset:window.mode==='stalePage'&&!c.offset?1:null,entries:[item]}};
 if(c.type==='reviewApiCountResolution'){const r={input:c.input,taskId:window.mode==='wrongTask'?'other-task':item.taskId,attemptId:item.attemptId,round:item.round,sourceDigest:'b'.repeat(64),policyDigest:'c'.repeat(64),previousLedgerHead:'d'.repeat(64),reviewDigest:'e'.repeat(64),maximumUsdNanos:item.maximumUsdNanos,originalOutcome:item.originalOutcome,generationDigest:'f'.repeat(64)};window.original=r;return {apiReview:r};}
 const r=window.original;
 if(c.type==='confirmApiCountResolution'){item.feeResolution={reviewDigest:r.reviewDigest,userEventDigest:'1'.repeat(64),state:'recorded',amountUsdNanos:r.input.amountUsdNanos};if(window.mode==='lost'||window.mode==='notFound')throw new Error('controlled acknowledgement lost');}
 return {apiConfirmation:{projectId:r.input.projectId,requestId:r.input.requestId,reviewDigest:r.reviewDigest,record:window.mode==='notFound'?null:{review:window.mode==='mismatch'?{...r,sourceDigest:'0'.repeat(64)}:r,userEventDigest:'1'.repeat(64),state:window.mode==='confirmedPending'?'confirmedPending':'recorded'}}};
}
window.reset=mode=>{item.feeResolution=null;item.originalOutcome=mode==='pending'?'pending':'unknown';window.mode=mode;window.commands=[];window.original=null;root.render(<ApiCountPanel key={++serial} query={query}/>);};window.reset('ok');`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:join(dir,'bundle'),emptyOutDir:false,lib:{entry,name:'ApiCountResolution',formats:['iife'],fileName:()=> 'ui.js'}}});
try {
 browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});page=await browser.newPage({viewport:{width:1200,height:1000}});page.setDefaultTimeout(10000);
 page.on('pageerror',e=>errors.push(e.message));await page.route('**/*',r=>{networkRequests++;return r.abort();});
 await page.setContent('<html><meta charset="utf-8"><body><div id="root"></div></body></html>');await page.addStyleTag({content:await readFile('apps/desktop/src/style.css','utf8')});await page.addScriptTag({content:await readFile(join(dir,'bundle/ui.js'),'utf8')});
 const button=name=>page.getByRole('button',{name,exact:true});
 async function reset(mode='ok'){await page.evaluate(m=>window.reset(m),mode);await button('读取 API 计数清单').click();}
 async function open(mode='ok'){await reset(mode);await button('核对此 API 计数费用').click();}
 async function fill(){await page.getByLabel('实际 API 计数金额（USD）',{exact:true}).fill('0');await page.getByLabel('API 计数账单引用',{exact:true}).fill('明确零金额账单');await page.getByLabel('API 计数核对说明',{exact:true}).fill('仅核对原 API 计数费用');}
 async function prepare(mode='ok'){await open(mode);await fill();await button('审阅 API 计数费用').click();}
 await button('读取 API 计数清单').waitFor();assert.equal(await page.evaluate(()=>window.commands.length),0);cases.push('mount performs no requests');
 await open();await button('审阅 API 计数费用').click();await page.getByRole('alert').waitFor();assert.equal(await page.evaluate(()=>window.commands.length),1);cases.push('blank fee is not inferred as zero');
 await fill();await button('审阅 API 计数费用').click();await page.getByLabel('API 计数费用审阅',{exact:true}).waitFor();const review=page.getByLabel('API 计数费用审阅',{exact:true});assert.match(await review.innerText(),/任务：task.*尝试：original-attempt.*轮次：2/);assert.match(await review.innerText(),/核销金额：\$0/);assert.match(await review.innerText(),/生成费用与原执行状态保留/);cases.push('review binds task attempt round and separates generation liability');
 await page.getByLabel('实际 API 计数金额（USD）',{exact:true}).fill('0.001');assert.equal(await button('确认本次 API 计数核销').count(),0);cases.push('editing invalidates review');
 await button('审阅 API 计数费用').click();await button('确认本次 API 计数核销').click();await page.getByRole('status').waitFor();assert.match(await page.getByRole('status').innerText(),/已记入账本/);assert.equal(await page.evaluate(()=>window.commands.filter(c=>c.type==='confirmApiCountResolution').length),1);
 await button('关闭 API 计数核对').click();await button('读取 API 计数清单').click();await button('读取原 API 费用确认').click();await page.getByLabel('原 API 费用确认',{exact:true}).waitFor();assert.match(await page.getByLabel('原 API 费用确认',{exact:true}).innerText(),/明确零金额账单/);cases.push('one confirmation and read-only original detail');
 await prepare('lost');await button('确认本次 API 计数核销').click();await page.getByRole('alert').waitFor();assert.equal(await button('确认本次 API 计数核销').count(),0);await button('只读查询原 API 计数确认').click();await page.getByRole('status').waitFor();assert.match(await page.getByRole('status').innerText(),/已记入账本/);assert.equal(await page.evaluate(()=>window.commands.filter(c=>c.type==='confirmApiCountResolution').length),1);cases.push('lost acknowledgement allows lookup without resend');
 await prepare('notFound');await button('确认本次 API 计数核销').click();await page.getByRole('alert').waitFor();await button('只读查询原 API 计数确认').click();await page.getByRole('status').waitFor();assert.match(await page.getByRole('status').innerText(),/尚未找到原确认/);assert.equal(await page.evaluate(()=>window.commands.filter(c=>c.type==='confirmApiCountResolution').length),1);cases.push('missing original confirmation does not trigger retry');
 await prepare('mismatch');await button('确认本次 API 计数核销').click();await page.getByRole('alert').waitFor();assert.match(await page.getByRole('alert').innerText(),/未能核实/);await button('只读查询原 API 计数确认').click();assert.match(await page.getByRole('alert').innerText(),/不符/);cases.push('changed source is not shown as success');
 await prepare('wrongTask');await page.getByRole('alert').waitFor();assert.equal(await button('确认本次 API 计数核销').count(),0);cases.push('wrong task review cannot be confirmed');
 await prepare('confirmedPending');await button('确认本次 API 计数核销').click();await page.getByRole('status').waitFor();assert.match(await page.getByRole('status').innerText(),/账本待发布/);cases.push('saved user event remains visibly pending publication');
 await reset('pending');assert.equal(await button('核对此 API 计数费用').isDisabled(),true);cases.push('pending count has no reconciliation action');
 await reset('stalePage');await button('下一页 API 计数').click();await page.getByRole('alert').waitFor();assert.match(await page.getByRole('alert').innerText(),/清单回复不一致/);cases.push('changed page revision requires rereading');
 await prepare();await page.getByLabel('API 计数费用审阅',{exact:true}).waitFor();await page.screenshot({path:join(dir,'api-count-review-desktop.png'),fullPage:true});await page.setViewportSize({width:412,height:915});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth),true);await page.screenshot({path:join(dir,'api-count-review-narrow.png'),fullPage:true});cases.push('long original request and review fit narrow screen');
 assert.deepEqual(errors,[]);assert.equal(networkRequests,0);await writeFile(join(dir,'commands.json'),JSON.stringify(await page.evaluate(()=>window.allCommands),null,2));await writeFile(join(dir,'result.json'),JSON.stringify({status:'passed',cases,scope:'production React with controlled replies; no native IPC or installed app',networkRequests,pageErrors:errors,modelCalls:0,artifact:dir},null,2));console.log(JSON.stringify({status:'passed',cases:cases.length,artifact:dir}));
}catch(e){await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(e),stack:e.stack,cases,errors,networkRequests},null,2));throw e;}finally{await browser?.close();}
