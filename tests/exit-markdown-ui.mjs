// Production React components with offline recording sinks; no native IPC or providers.
import {mkdtemp,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
import {build} from 'vite';
import {chromium} from 'playwright-core';
const dir=await mkdtemp(resolve('reports/exit-markdown-ui-776-'));
const entry=join(dir,'entry.tsx');
await writeFile(entry,`import React from 'react';import {createRoot} from 'react-dom/client';import {flushSync} from 'react-dom';import {ExitWorkbench} from '../../apps/desktop/src/ExitWorkbench';
const root=createRoot(document.getElementById('root'));let epoch=0;window.exitCalls=[];
const api={listen:async h=>{window.closeRequested=h;return()=>{window.closeRequested=undefined;}},stop:async()=>{window.exitCalls.push('stop');return new Promise((resolve,reject)=>window.stopResult={resolve,reject});},cancel:async()=>{window.exitCalls.push('cancel');}};
window.show=()=>flushSync(()=>root.render(<ExitWorkbench key={++epoch} enabled api={api}/>));`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:join(dir,'bundle'),emptyOutDir:false,lib:{entry,name:'ExitMarkdown',formats:['iife'],fileName:()=> 'ui.js'}}});
const cases=[],errors=[];let browser,page,network=0;
// Markdown scenarios now live in markdown-refinement-ui.mjs.
try{
 browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
 page=await browser.newPage({viewport:{width:1060,height:900}});page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push(String(e)));
 await page.route('**/*',route=>{network++;return route.abort();});await page.setContent('<html lang="zh-CN"><meta charset="utf-8"><main style="padding:24px;max-width:960px;margin:auto"><div id="root"></div></main></html>');
 await page.addStyleTag({path:resolve('apps/desktop/src/style.css')});await page.addScriptTag({path:join(dir,'bundle/ui.js')});
 await page.evaluate(()=>window.show('exit'));await page.waitForFunction(()=>!!window.closeRequested);await page.evaluate(()=>window.closeRequested());
 await page.getByRole('dialog').waitFor();assert.deepEqual(await page.evaluate(()=>window.exitCalls),[]);
 await page.getByRole('button',{name:'返回工作台',exact:true}).click();assert.equal(await page.getByRole('dialog').count(),0);assert.deepEqual(await page.evaluate(()=>window.exitCalls),['cancel']);
 cases.push('Native close event asks for confirmation; returning leaves the core running');
 await page.getByRole('button',{name:'退出工作台',exact:true}).click();await page.getByRole('button',{name:'确认退出并停止后台'}).click();
 assert.equal(await page.getByRole('button',{name:'确认退出并停止后台'}).isDisabled(),true);assert.equal(await page.getByRole('button',{name:'返回工作台',exact:true}).isDisabled(),true);
 await page.evaluate(()=>window.closeRequested());assert.deepEqual(await page.evaluate(()=>window.exitCalls),['cancel','stop']);
 await page.evaluate(()=>window.stopResult.reject(new Error('仍有未决任务，未退出核心')));await page.getByRole('alert').filter({hasText:'未决任务'}).waitFor();assert.equal(await page.getByRole('dialog').count(),1);
 await page.screenshot({path:join(dir,'exit-blocked.png'),fullPage:true});
 cases.push('Exit waits for backend result, suppresses duplicate clicks and retains window on a rejected stop');
 await page.getByRole('button',{name:'返回工作台',exact:true}).click();await page.getByRole('button',{name:'退出工作台',exact:true}).click();await page.getByRole('button',{name:'确认退出并停止后台'}).click();await page.evaluate(()=>window.stopResult.resolve());
 assert.deepEqual(await page.evaluate(()=>window.exitCalls),['cancel','stop','cancel','stop']);
 cases.push('A later user-confirmed exit uses one backend request; frontend never bypasses native shutdown with direct window close');
 assert.equal(network,0);assert.deepEqual(errors,[]);
 await writeFile(join(dir,'result.json'),JSON.stringify({status:'passed',cases,networkRequests:network,errors,providerCalls:0},null,2));console.log(JSON.stringify({status:'passed',cases:cases.length,report:dir}));
}catch(e){await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(e),cases,errors,network},null,2));await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});throw e;}
finally{await browser?.close();}
