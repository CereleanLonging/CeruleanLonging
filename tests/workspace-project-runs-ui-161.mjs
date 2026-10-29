import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { build } from 'vite';
import { chromium } from 'playwright-core';

if (process.env.CERULEAN_NATIVE_OWNED_JOB !== '1') throw Error('Use scripts/run-workspace-project-runs-ui-161.ps1 owned runner');
const dir = await mkdtemp('reports/workspace-project-runs-ui-161-');
const cases = [], errors = [];
let browser, page, networkRequests = 0;
const files = [
  'apps/desktop/src/WorkspaceBrowser.tsx',
  'apps/desktop/src/WorkspaceProjectRuns.tsx',
  'apps/desktop/src/contracts.ts',
  'tests/workspace-project-runs-ui-161.mjs',
  'scripts/run-workspace-project-runs-ui-161.ps1',
];
const hashes = async () => Object.fromEntries(await Promise.all(files.map(async path => [path, createHash('sha256').update(await readFile(path)).digest('hex')])));
const before = await hashes();
const entry = join(dir, 'entry.tsx');
await writeFile(entry, `import React from 'react';import {createRoot} from 'react-dom/client';import {WorkspaceBrowser} from '../../apps/desktop/src/WorkspaceBrowser';import {WorkspaceProjectRuns} from '../../apps/desktop/src/WorkspaceProjectRuns';
const root=createRoot(document.getElementById('root'));window.epoch=0;
window.read=async q=>{
window.reads.push(q);if(q.type==='overview')return {type:q.type,value:{product:'local',protocolVersion:1,dataDirectory:'original',mode:'ready',legacyPlanCount:0,pendingLegacyPlanCount:0,projectCount:2,message:null}};
if(q.type==='plans')return {type:q.type,value:{revision:'plans-v1',offset:0,total:0,nextOffset:null,entries:[]}};
if(q.type==='projects')return {type:q.type,value:{revision:'projects-v1',offset:0,total:2,nextOffset:null,entries:['p','other'].map(id=>({project:{id,root:id,timezone:'UTC'},revisionDigest:'plan-'+id,sequence:1,approved:false,taskCount:1,resourceHoldingTasks:1,unknownTasks:1}))}};
if(!['projectRuns','projectRun'].includes(q.type))throw Error('unexpected read '+q.type);
window.active++;window.maxActive=Math.max(window.maxActive,window.active);
try{if(window.fail)throw Error('original project-run record unavailable');const projectId=q.projectId;let value;if(q.type==='projectRuns'){const rows=window.rows.filter(run=>run.projectId===projectId);const entries=rows.slice(q.offset,q.offset+20).map(run=>({...run,hasMessage:run.message.length>0}));value={projectId,revision:window.revision,offset:q.offset,total:rows.length,nextOffset:q.offset+20<rows.length?q.offset+20:null,activeCount:window.activeCount,entries};}else{const record=window.rows.find(run=>run.id===q.runId);value={projectId,revision:window.revision,record};}if(window.patch)value={...value,...window.patch};const result={type:q.type,value};if(window.hold){window.hold=false;return await new Promise(resolve=>{window.finish=()=>resolve(result)})}return result;}finally{window.active--}
};
window.render=()=>root.render(<React.StrictMode><div data-epoch={window.epoch}>{window.direct?<WorkspaceProjectRuns key={window.epoch} projectId='p' read={window.read} busy={!!window.busy}/>:<WorkspaceBrowser key={window.epoch} enabled={true} busy={!!window.busy} showPlans={false} send={async c=>{window.sends.push(c);throw Error('unexpected mutation')}} onOverview={()=>{}} read={window.read}/>}</div></React.StrictMode>);
window.set=s=>{window.epoch++;Object.assign(window,{reads:[],sends:[],active:0,maxActive:0,fail:false,patch:null,hold:false,finish:null,busy:false,direct:false},s);window.render()};`);
await build({ configFile:false, logLevel:'error', define:{'process.env.NODE_ENV':JSON.stringify('production')}, oxc:{jsx:{runtime:'automatic'}}, build:{outDir:join(dir,'bundle'),emptyOutDir:false,lib:{entry,name:'ProjectRunsUI',formats:['iife'],fileName:()=> 'ui.js'}} });

const originalRows = Array.from({length:22},(_,index)=>({id:'run-'+index,projectId:'p',revisionDigest:'revision-'+index,setupDigest:'setup-'+index,state:'paused',message:index===0?'ORIGINAL ACTIVE MESSAGE':'ORIGINAL MESSAGE '+index,acknowledgedDecisions:index}));
const originalDetail = originalRows[0];
try {
  browser = await chromium.launch({ executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless:true });
  page = await browser.newPage(); page.setDefaultTimeout(10000); await page.clock.install();
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/*', route => { networkRequests++; return route.abort(); });
  await page.setContent('<html lang="zh-CN"><div id="root"></div></html>');
  await page.addScriptTag({ content:await readFile(join(dir,'bundle/ui.js'),'utf8') });
  const region = () => page.getByRole('region',{name:'项目运行记录',exact:true});
  const nav = () => region().getByRole('navigation',{name:'项目运行分页',exact:true});
  const detailRegion = () => region().getByRole('region',{name:'完整原项目运行记录',exact:true});
  const ready = async (total=22) => { await region().getByText(new RegExp(`原项目共 ${total} 条运行记录`)).waitFor(); await page.waitForFunction(() => !Array.from(document.querySelectorAll('button')).find(button => button.textContent === '刷新运行首页')?.disabled); };
  const reset = async extra => {
    await page.evaluate(value=>window.set(value),{rows:originalRows,revision:'runs-v1',activeCount:0,...extra});
    await page.waitForFunction(() => document.querySelector('[data-epoch]')?.getAttribute('data-epoch')===String(window.epoch));
    await page.getByRole('button',{name:'查看项目 p 的运行记录',exact:true}).click(); await ready(extra?.expectedTotal ?? 22);
  };
  const inspect = async () => { await region().getByRole('button',{name:'查看完整原运行记录',exact:true}).first().click(); await detailRegion().getByText('ORIGINAL ACTIVE MESSAGE',{exact:true}).waitFor(); };
  await page.evaluate(value=>window.set(value),{rows:originalRows,revision:'runs-v1',activeCount:0,busy:true,direct:true});
  await page.waitForFunction(() => document.querySelector('[data-epoch]')?.getAttribute('data-epoch')===String(window.epoch));
  await region().getByText('项目运行记录尚未取得；这不表示项目没有运行。',{exact:true}).waitFor();
  assert.equal(await page.evaluate(()=>window.reads.filter(q=>q.type==='projectRuns').length),0);
  await page.evaluate(()=>{window.busy=false;window.render()}); await ready();
  assert.deepEqual(await page.evaluate(()=>window.reads.at(-1)),{type:'projectRuns',projectId:'p',offset:0,revision:null});
  cases.push('mounted project-run panel defers its initial read while busy and reads after the busy guard clears');
  cases.push('initial project-run read blocked by busy resumes after busy clears without being inferred empty');

  await reset();
  assert.deepEqual(await page.evaluate(()=>window.reads.at(-1)),{type:'projectRuns',projectId:'p',offset:0,revision:null});
  assert.equal(await region().getByRole('button',{name:'查看完整原运行记录',exact:true}).count(),20);
  await inspect();
  assert.deepEqual(await page.evaluate(()=>window.reads.at(-1)),{type:'projectRun',projectId:'p',runId:'run-0',revision:'runs-v1'});
  assert.match(await detailRegion().innerText(),/revision-0/); assert.match(await detailRegion().innerText(),/setup-0/);
  await detailRegion().getByText('查看原记录字段',{exact:true}).click();
  assert.deepEqual(JSON.parse(await detailRegion().locator('pre').innerText()),originalDetail); await page.screenshot({path:join(dir,'project-runs-detail.png'),fullPage:true});
  assert.equal(await region().getByRole('button',{name:/启动|暂停|取消|恢复/}).count(),0);
  cases.push('selected-project entry performs exact bounded page/detail reads and renders original IDs, digests, message and decisions read-only');

  await nav().getByRole('button',{name:'下一页'}).click();
  await region().getByRole('button',{name:'查看完整原运行记录',exact:true}).first().waitFor();
  assert.deepEqual(await page.evaluate(()=>window.reads.at(-1)),{type:'projectRuns',projectId:'p',offset:20,revision:'runs-v1'});
  assert.equal(await region().getByRole('button',{name:'查看完整原运行记录',exact:true}).count(),2);
  assert.equal(await nav().getByRole('button',{name:'下一页'}).isDisabled(),true);
  await nav().getByRole('button',{name:'上一页'}).click(); await ready();
  cases.push('pages are limited to 20 and forward/back cursors retain project identity and pinned revision');
  await reset(); await inspect(); await page.evaluate(()=>{window.activeCount=2;window.hold=true});
  await region().getByRole('button',{name:'刷新运行首页'}).click(); await page.waitForFunction(()=>typeof window.finish==='function'); await page.evaluate(()=>window.finish()); await region().getByRole('alert').waitFor(); assert.match(await detailRegion().innerText(),/ORIGINAL ACTIVE MESSAGE/);
  cases.push('same-revision active-count inconsistency is refused while old page and detail remain visible');
  await reset({activeCount:1,rows:originalRows.map((run,index)=>index===0?{...run,state:'active'}:run)}); await inspect(); await page.evaluate(()=>{window.rows=window.rows.map(run=>({...run,state:'paused'}));window.activeCount=0;window.revision='runs-v2';});
  const beforePoll=await page.evaluate(()=>window.reads.filter(q=>q.type==='projectRuns').length), beforeAllReads=await page.evaluate(()=>window.reads.length); await page.clock.runFor(1100); await region().getByText('版本 runs-v2',{exact:false}).waitFor();
  assert.equal(await page.evaluate(()=>window.reads.filter(q=>q.type==='projectRuns').length),beforePoll+1); assert.equal(await page.evaluate(()=>window.reads.length),beforeAllReads+1); assert.deepEqual(await page.evaluate(()=>window.reads.at(-1)),{type:'projectRuns',projectId:'p',offset:0,revision:null});
  assert.equal(await detailRegion().count(),0);
  await page.clock.runFor(10000); assert.equal(await page.evaluate(()=>window.reads.filter(q=>q.type==='projectRuns').length),beforePoll+1);
  cases.push('active_count polling refreshes only the first page with revision null, invalidates old detail on version change and stops at paused');

  await reset(); await inspect(); await page.evaluate(()=>{window.hold=true});
  await region().getByRole('button',{name:'查看完整原运行记录',exact:true}).nth(1).click(); await page.waitForFunction(()=>typeof window.finish==='function');
  await detailRegion().getByRole('button',{name:'关闭'}).click(); await page.evaluate(()=>window.finish()); assert.equal(await detailRegion().count(),0);
  await reset(); await page.evaluate(()=>{window.hold=true});
  await region().getByRole('button',{name:'查看完整原运行记录',exact:true}).first().evaluate(button=>{button.click();button.click()});
  await page.waitForFunction(()=>typeof window.finish==='function');
  assert.equal(await page.evaluate(()=>window.reads.filter(q=>q.type==='projectRun').length),1); assert.equal(await page.evaluate(()=>window.maxActive),1);
  await page.evaluate(()=>window.finish()); await page.clock.runFor(2000); assert.equal(await page.evaluate(()=>window.maxActive),1);
  cases.push('closing an in-flight detail invalidates its late response; repeated detail actions are lock-serialized without duplicate reads or concurrent polling');

  await reset(); await inspect(); await page.evaluate(()=>{window.fail=true});
  await nav().getByRole('button',{name:'下一页'}).click(); await region().getByRole('alert').waitFor();
  assert.match(await region().innerText(),/ORIGINAL ACTIVE MESSAGE/); assert.match(await region().innerText(),/runs-v1/); assert.equal(await nav().getByRole('button',{name:'下一页'}).isDisabled(),true);
  const failed=await page.evaluate(()=>window.reads.length); await page.clock.runFor(10000); assert.equal(await page.evaluate(()=>window.reads.length),failed);
  await page.evaluate(()=>window.fail=false); await region().getByRole('button',{name:'刷新运行首页'}).click(); await ready();
  cases.push('stale/error page keeps prior page and detail evidence, blocks continuation and stops automatic polling until explicit refresh');

  for(const patch of [{projectId:'foreign'},{revision:'foreign'},{offset:1},{total:99},{nextOffset:7},{entries:[]},{entries:Array.from({length:21},()=>({...originalRows[0],hasMessage:true}))},{entries:[{...originalRows[0],hasMessage:true},{...originalRows[0],hasMessage:true}]}]){
    await reset(); await inspect(); await page.evaluate(value=>window.patch=value,patch); await nav().getByRole('button',{name:'下一页'}).click(); await region().getByRole('alert').waitFor(); assert.match(await detailRegion().innerText(),/ORIGINAL ACTIVE MESSAGE/);
  }
  cases.push('foreign identity, revision, cursor, total, empty, oversized and duplicate page responses are refused without replacing prior evidence');

  await reset(); await page.evaluate(()=>window.hold=true); await region().getByRole('button',{name:'查看完整原运行记录',exact:true}).first().click(); await page.waitForFunction(()=>typeof window.finish==='function');
  await page.getByRole('button',{name:'查看项目 other 的运行记录',exact:true}).click(); await page.getByRole('heading',{name:'项目 other 的运行记录'}).waitFor(); await page.evaluate(()=>window.finish());
  assert.equal(await page.getByRole('region',{name:'完整原项目运行记录',exact:true}).count(),0);
  cases.push('project switch invalidates late detail responses and cannot leak evidence across selected projects');

  await reset(); await page.evaluate(()=>window.hold=true); await region().getByRole('button',{name:'刷新运行首页'}).click(); await page.waitForFunction(()=>typeof window.finish==='function'); await page.getByRole('button',{name:'关闭项目运行记录'}).click(); await page.evaluate(()=>window.finish()); await page.clock.runFor(3000); assert.equal(await page.getByRole('region',{name:'项目运行记录',exact:true}).count(),0);
  cases.push('unmount invalidates late page responses and prevents remount or stale updates');

  await page.evaluate(value=>window.set(value),{rows:[],revision:'empty',activeCount:0}); await page.waitForFunction(()=>document.querySelector('[data-epoch]')?.getAttribute('data-epoch')===String(window.epoch)); await page.getByRole('button',{name:'查看项目 p 的运行记录',exact:true}).click(); await region().getByText('原项目尚无运行记录。',{exact:true}).waitFor();
  await page.evaluate(value=>window.set(value),{rows:originalRows,revision:'missing',activeCount:0,fail:true}); await page.waitForFunction(()=>document.querySelector('[data-epoch]')?.getAttribute('data-epoch')===String(window.epoch)); await page.getByRole('button',{name:'查看项目 p 的运行记录',exact:true}).click(); await region().getByRole('alert').waitFor(); assert.match(await region().innerText(),/尚未取得/); assert.doesNotMatch(await region().innerText(),/尚无运行记录/);
  cases.push('explicit empty is distinct from unavailable initial reads and never inferred from refusal');

  assert.equal(await page.evaluate(()=>window.sends.length),0); assert.equal(await page.evaluate(()=>window.reads.some(q=>!['overview','plans','projects','projectRuns','projectRun'].includes(q.type))),false); assert.deepEqual(errors,[]); assert.equal(networkRequests,0);
  const after=await hashes(); assert.deepEqual(after,before);
  await writeFile(join(dir,'result.json'),JSON.stringify({status:'passed',cases,sourceBefore:before,sourceAfter:after,pageErrors:errors,networkRequests,modelCalls:0,mutations:0,schema:{pageQuery:{type:'projectRuns',projectId:'p',offset:0,revision:null},detailQuery:{type:'projectRun',projectId:'p',runId:'run-0',revision:'runs-v1'},pageReply:'{type,value:WorkspaceProjectRunPage}',detailReply:'{type,value:WorkspaceProjectRunDetail}',pageLimit:20},scope:'Production WorkspaceBrowser and WorkspaceProjectRuns with controlled offline reads; not installed Tauri'} ,null,2));
  console.log(JSON.stringify({status:'passed',cases:cases.length,artifact:dir}));
} catch(error) {
  await writeFile(join(dir,'result.json'),JSON.stringify({status:'failed',cases,error:String(error),pageErrors:errors,networkRequests},null,2)); throw error;
} finally { await browser?.close(); }
