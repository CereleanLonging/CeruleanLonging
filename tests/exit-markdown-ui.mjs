// Production React components with offline recording sinks; no native IPC or providers.
import {mkdtemp,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
import {build} from 'vite';
import {chromium} from 'playwright-core';
const dir=await mkdtemp(resolve('reports/exit-ui-785-'));
const entry=join(dir,'entry.tsx');
await writeFile(entry,`import React from 'react';import {createRoot} from 'react-dom/client';import {flushSync} from 'react-dom';import {ExitWorkbench} from '../../apps/desktop/src/ExitWorkbench';
const root=createRoot(document.getElementById('root'));let epoch=0;window.exitCalls=[];
const api={listen:async h=>{window.closeRequested=h;return()=>{window.closeRequested=undefined;}},stop:async()=>{window.exitCalls.push('stop');return new Promise((resolve,reject)=>window.stopResult={resolve,reject});},cancel:async()=>{window.exitCalls.push('cancel');}};
window.backgroundClicks=0;
window.show=()=>flushSync(()=>root.render(<React.StrictMode><div className="shell"><aside className="sidebar"><div className="sidebar-footer"><ExitWorkbench key={++epoch} enabled api={api}/></div></aside><main><section className="panel"><h2>项目清单</h2><p>正常页面内容</p></section><nav className="workspace-pagination" aria-label="项目分页" style={{position:'relative',zIndex:20,marginTop:300}}><button disabled className="secondary-button">上一页</button><span>当前显示 1 / 2</span><button className="secondary-button" onClick={()=>window.backgroundClicks++}>下一页</button></nav></main></div></React.StrictMode>));`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:join(dir,'bundle'),emptyOutDir:false,lib:{entry,name:'ExitMarkdown',formats:['iife'],fileName:()=> 'ui.js'}}});
const cases=[],errors=[];let browser,page,network=0;
// Markdown scenarios now live in markdown-refinement-ui.mjs.
try{
 browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
 page=await browser.newPage({viewport:{width:1060,height:900}});page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push(String(e)));
 await page.route('**/*',route=>{network++;return route.abort();});await page.setContent('<html lang="zh-CN"><meta charset="utf-8"><div id="root"></div></html>');
 await page.addStyleTag({path:resolve('apps/desktop/src/style.css')});await page.addScriptTag({path:join(dir,'bundle/ui.js')});
 await page.evaluate(()=>window.show('exit'));await page.waitForFunction(()=>!!window.closeRequested);await page.evaluate(()=>window.closeRequested());
 await page.getByRole('dialog').waitFor();assert.deepEqual(await page.evaluate(()=>window.exitCalls),[]);
 await page.getByRole('button',{name:'返回工作台',exact:true}).click();assert.equal(await page.getByRole('dialog').count(),0);assert.deepEqual(await page.evaluate(()=>window.exitCalls),['cancel']);
 cases.push('Native close event asks for confirmation; returning leaves the core running');
 await page.getByRole('button',{name:'退出工作台',exact:true}).click();
 assert.equal(await page.locator('dialog').evaluate(d=>d.matches(':modal')),true);
 const pagination=page.locator('.workspace-pagination');
 for(const button of await pagination.locator('button').all()){
   const b=await button.boundingBox();assert(b);
   assert.equal(await page.evaluate(({x,y,width,height})=>!!document.elementFromPoint(x+width/2,y+height/2)?.closest('dialog'),b),true);
   await page.mouse.click(b.x+b.width/2,b.y+b.height/2);
 }
 assert.equal(await page.evaluate(()=>window.backgroundClicks),0);
 await pagination.locator('button').last().evaluate(b=>b.focus());
 assert.equal(await page.evaluate(()=>!!document.activeElement?.closest('dialog')),true);
 for(const key of ['Tab','Tab','Shift+Tab','Shift+Tab','Tab']){await page.keyboard.press(key);assert.equal(await page.evaluate(()=>!!document.activeElement?.closest('main,aside')),false);}
 await page.screenshot({path:join(dir,'exit-modal-desktop.png'),fullPage:true});
 cases.push('Top-layer dialog covers disabled and elevated pagination, blocks background clicks and confines focus');
 if(await page.getByRole('tooltip').count()) { await page.keyboard.press('Escape');assert.equal(await page.getByRole('tooltip').count(),0);assert.equal(await page.locator('dialog').evaluate(d=>d.matches(':modal')),true); }
 await page.keyboard.press('Escape');await page.getByRole('dialog').waitFor({state:'detached'});
 assert.deepEqual(await page.evaluate(()=>window.exitCalls),['cancel','cancel']);
 assert.equal(await page.getByRole('button',{name:'退出工作台',exact:true}).evaluate(b=>document.activeElement===b),true);
 await page.getByRole('button',{name:'下一页',exact:true}).click();assert.equal(await page.evaluate(()=>window.backgroundClicks),1);
 cases.push('Escape follows native cancel protocol, restores trigger focus and re-enables the page');
 await page.getByRole('button',{name:'退出工作台',exact:true}).click();await page.getByRole('button',{name:'确认退出并停止后台'}).click();
 assert.equal(await page.getByRole('button',{name:'确认退出并停止后台'}).isDisabled(),true);assert.equal(await page.getByRole('button',{name:'返回工作台',exact:true}).isDisabled(),true);
 await page.keyboard.press('Escape');await page.evaluate(()=>window.closeRequested());assert.deepEqual(await page.evaluate(()=>window.exitCalls),['cancel','cancel','stop']);assert.equal(await page.locator('dialog').evaluate(d=>d.matches(':modal')),true);
 await page.evaluate(()=>window.stopResult.reject(new Error('仍有未决任务，未退出核心')));await page.getByRole('alert').filter({hasText:'未决任务'}).waitFor();assert.equal(await page.getByRole('dialog').count(),1);
 await page.screenshot({path:join(dir,'exit-blocked.png'),fullPage:true});
 cases.push('Exit waits for backend result, suppresses duplicate clicks and retains window on a rejected stop');
 await page.setViewportSize({width:390,height:700});
 const bounds=await page.getByRole('dialog').boundingBox();assert(bounds&&bounds.x>=0&&bounds.y>=0&&bounds.x+bounds.width<=390&&bounds.y+bounds.height<=700);
 await page.screenshot({path:join(dir,'exit-modal-small.png'),fullPage:true});
 cases.push('Dialog fits a narrow viewport and keeps both actions reachable after stop rejection');
 await page.getByRole('button',{name:'返回工作台',exact:true}).click();await page.getByRole('button',{name:'退出工作台',exact:true}).click();await page.getByRole('button',{name:'确认退出并停止后台'}).click();await page.evaluate(()=>window.stopResult.resolve());
 assert.deepEqual(await page.evaluate(()=>window.exitCalls),['cancel','cancel','stop','cancel','stop']);
 cases.push('A later user-confirmed exit uses one backend request; frontend never bypasses native shutdown with direct window close');
 assert.equal(network,0);assert.deepEqual(errors,[]);
 await writeFile(join(dir,'result.json'),JSON.stringify({status:'passed',cases,networkRequests:network,errors,providerCalls:0},null,2));console.log(JSON.stringify({status:'passed',cases:cases.length,report:dir}));
}catch(e){await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(e),cases,errors,network},null,2));await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});throw e;}
finally{await browser?.close();}
