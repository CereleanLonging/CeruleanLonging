// Production React account view with controlled durable-count snapshots only.
import {readFile,writeFile,mkdtemp} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {build} from 'vite';
import {chromium} from 'playwright-core';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const dir=await mkdtemp(resolve('reports/api-count-status-ui-184-'));
const files=['CurrencyAccounts.tsx','CurrencyHistory.tsx','CurrencyBudgetFields.tsx','contracts.ts'].map(n=>'apps/desktop/src/'+n);
async function hashes(){return Object.fromEntries(await Promise.all(files.map(async p=>[p,createHash('sha256').update(await readFile(p)).digest('hex')])));}
const before=await hashes(),cases=[],errors=[];let networkRequests=0,browser,page;
const entry=join(dir,'entry.tsx');
await writeFile(entry,`import React from 'react';import {createRoot} from 'react-dom/client';import {CurrencyAccounts} from '../../apps/desktop/src/CurrencyAccounts';const root=createRoot(document.getElementById('root')!);window.renderAccounts=accounts=>root.render(<CurrencyAccounts accounts={accounts} history={[]}/>);window.renderAccounts([]);`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:join(dir,'bundle'),emptyOutDir:false,lib:{entry,name:'CountStatus',formats:['iife'],fileName:()=> 'ui.js'}}});
try {
 browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
 page=await browser.newPage({viewport:{width:1200,height:1000}});page.setDefaultTimeout(10000);
 page.on('pageerror',e=>errors.push(e.message));await page.route('**/*',r=>{networkRequests++;return r.abort();});
 await page.setContent('<html><meta charset="utf-8"><body><div id="root"></div></body></html>');
 await page.addStyleTag({content:await readFile('apps/desktop/src/style.css','utf8')});
 await page.addScriptTag({content:await readFile(join(dir,'bundle/ui.js'),'utf8')});
 const ready={projectId:'project',planningSetupDigest:'a'.repeat(64),revisionDigest:'b'.repeat(64),ledgerHead:'c'.repeat(64),limitUsdNanos:10000000000,chargedUsdNanos:100000000,reservedUsdNanos:1000000,availableUsdNanos:9899000000,approvalLinked:true,healthy:true,blocked:false,unknownRequests:0,unpricedRequests:0};
 const count={pending:1,counted:0,unknown:0,awaitingGeneration:0,reservedUsdNanos:1000000,needsReview:false};
 const panel=page.getByRole('article',{name:'项目 project 金额'});
 async function show(c){await page.evaluate(a=>window.renderAccounts([a]),{...ready,reservedUsdNanos:1000000+(c?.reservedUsdNanos??0),availableUsdNanos:9899000000-(c?.reservedUsdNanos??0),managementCounts:count,...(c?{apiCounts:c}:{})});await panel.waitFor();}
 const api={pending:1,counted:0,unknown:0,countedWithoutGeneration:0,reservedUsdNanos:1000};
 await show();assert.equal(await page.getByLabel('API 执行计数记录').count(),0);cases.push('legacy snapshot omits unavailable API count summary');
 await show(api);const apiPanel=page.getByLabel('API 执行计数记录');await apiPanel.getByText(/API 计数等待结果：1/).waitFor();
 assert.match(await apiPanel.innerText(),/API 计数费用预留：\$0\.000001，已包含/);
 assert.equal(await panel.getByRole('alert').count(),0);cases.push('pending API count exposes included reservation without false pause');
 assert.match(await page.getByLabel('指挥计数记录').innerText(),/计数费用预留：\$0\.001，已包含/);
 assert.equal(await panel.locator('dt').filter({hasText:'未释放预留'}).evaluate(e=>e.nextElementSibling.textContent),'$0.001001');
 cases.push('management and API summaries stay separate and total is not incremented');
 await show({...api,pending:0,counted:1});await apiPanel.getByText(/已取得收据：1/).waitFor();
 assert.match(await apiPanel.innerText(),/生成完成不代表计数免费/);cases.push('counted receipt does not imply settled count fee');
 await show({...api,pending:0,unknown:1});await apiPanel.getByText(/结果未知：1/).waitFor();
 assert.match(await apiPanel.innerText(),/API 计数费用核对/);
 assert.match(await apiPanel.innerText(),/指挥计数核对不处理这些记录/);cases.push('unknown fee retains liability and points to separate API reconciliation');
 await show({...api,pending:0,counted:1,countedWithoutGeneration:1});await apiPanel.getByText(/已计数但尚无生成预留：1/).waitFor();
 assert.equal(await panel.getByRole('button').count(),0);
 assert.match(await apiPanel.innerText(),/不会重新计数、生成或释放预留/);cases.push('unprepared generation exposes no resend or refund control');
 await page.screenshot({path:join(dir,'count-status-desktop.png'),fullPage:true});
 await page.setViewportSize({width:412,height:915});
 assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth),true);
 await page.screenshot({path:join(dir,'count-status-narrow.png'),fullPage:true});cases.push('count state and fee fit narrow window');
 assert.deepEqual(errors,[]);assert.equal(networkRequests,0);const after=await hashes();assert.deepEqual(after,before);
 await writeFile(join(dir,'result.json'),JSON.stringify({status:'passed',cases,scope:'production React with synthetic snapshot; no IPC or model',sourceBefore:before,sourceAfter:after,pageErrors:errors,networkRequests,modelCalls:0,artifact:dir},null,2));
 console.log(JSON.stringify({status:'passed',cases:cases.length,artifact:dir}));
} catch(e) {await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(e),stack:e.stack,cases,errors,networkRequests},null,2));throw e;
} finally {await browser?.close();}
