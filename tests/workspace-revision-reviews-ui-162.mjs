import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { build } from 'vite';
import { chromium } from 'playwright-core';

if (process.env.CERULEAN_NATIVE_OWNED_JOB !== '1') throw Error('Use scripts/run-workspace-revision-reviews-ui-162.ps1 owned runner');
await mkdir('reports/workspace-revision-reviews-162', { recursive: true });
const dir = await mkdtemp('reports/workspace-revision-reviews-162/ui-');
const cases = [], errors = [];
let browser, page, networkRequests = 0, mutations = 0;
const files = ['apps/desktop/src/WorkspaceBrowser.tsx', 'apps/desktop/src/WorkspaceRevisionReviews.tsx', 'apps/desktop/src/contracts.ts', 'tests/workspace-revision-reviews-ui-162.mjs', 'scripts/run-workspace-revision-reviews-ui-162.ps1'];
const hashes = async () => Object.fromEntries(await Promise.all(files.map(async path => [path, createHash('sha256').update(await readFile(path)).digest('hex')])));
const before = await hashes();
const entry = join(dir, 'entry.tsx');
await writeFile(entry, `import React from 'react';import {createRoot} from 'react-dom/client';import {WorkspaceBrowser} from '../../../apps/desktop/src/WorkspaceBrowser';import {WorkspaceRevisionReviews} from '../../../apps/desktop/src/WorkspaceRevisionReviews';
const root=createRoot(document.getElementById('root'));window.epoch=0;
window.read=async q=>{
window.reads.push(q);window.allReads.push(q);if(q.type==='overview')return {type:q.type,value:{product:'local',protocolVersion:1,dataDirectory:'original',mode:'ready',legacyPlanCount:0,pendingLegacyPlanCount:0,projectCount:2,message:null}};
if(q.type==='plans')return {type:q.type,value:{revision:'plans-v1',offset:0,total:0,nextOffset:null,entries:[]}};
if(q.type==='projects')return {type:q.type,value:{revision:'projects-v1',offset:0,total:2,nextOffset:null,entries:['p','other'].map(id=>({project:{id,root:id,timezone:'UTC'},revisionDigest:'plan-'+id,sequence:1,approved:false,taskCount:1,resourceHoldingTasks:1,unknownTasks:1}))}};
if(!['revisionReviews','revisionReview'].includes(q.type))throw Error('unexpected read '+q.type);
window.active++;window.maxActive=Math.max(window.maxActive,window.active);
try{if(window.fail)throw Error(window.fail);let value;if(q.type==='revisionReviews'){const entries=window.unavailableReason!==null?[]:window.rows.filter(record=>record.projectId===q.projectId);value={projectId:q.projectId,revision:window.revision,offset:q.offset,total:entries.length,nextOffset:null,entries,unavailableReason:window.unavailableReason};}else value={projectId:q.projectId,revision:window.revision,record:window.rows.find(record=>record.projectId===q.projectId)};if(window.patch)value={...value,...window.patch};const result={type:q.type,value};if(window.hold){window.hold=false;return await new Promise(resolve=>{window.finish=()=>resolve(result)})}return result;}finally{window.active--;}
};
window.totalMutations=0;window.allReads=[];
window.render=()=>root.render(<React.StrictMode><div data-epoch={window.epoch}>{window.direct?<WorkspaceRevisionReviews key={window.epoch} projectId={window.projectId} read={window.read} busy={!!window.busy}/>:<WorkspaceBrowser key={window.epoch} enabled={true} busy={!!window.busy} showPlans={false} send={async c=>{window.totalMutations++;window.sends.push(c);throw Error('unexpected mutation')}} onOverview={()=>{}} read={window.read}/>}</div></React.StrictMode>);
window.set=s=>{window.epoch++;Object.assign(window,{reads:[],sends:[],active:0,maxActive:0,fail:false,patch:null,hold:false,finish:null,busy:false,direct:false,projectId:'p',unavailableReason:null},s);window.render()};`);
const original = { projectId:'p', revisionDigest:'ORIGINAL-REVISION', evidenceDigest:'ORIGINAL-EVIDENCE', acceptedTasks:1, cancelledTasks:2, retiredTasks:3, pendingTasks:4, submittedTasks:5, verifiedSubmissions:6 };
try {
  await build({ configFile:false, logLevel:'error', define:{'process.env.NODE_ENV':JSON.stringify('production')}, oxc:{jsx:{runtime:'automatic'}}, build:{outDir:join(dir,'bundle'),emptyOutDir:false,lib:{entry,name:'RevisionReviewsUI',formats:['iife'],fileName:()=> 'ui.js'}} });
  browser = await chromium.launch({ executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless:true });
  page = await browser.newPage(); page.setDefaultTimeout(10000); await page.clock.install();
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/*', route => { networkRequests++; return route.abort(); });
  await page.setContent('<html lang="zh-CN"><div id="root"></div></html>');
  await page.addScriptTag({ content:await readFile(join(dir,'bundle/ui.js'),'utf8') });
  const region = () => page.getByRole('region',{name:'项目修订审阅',exact:true});
  const detail = () => page.getByRole('region',{name:'完整原项目修订审阅',exact:true});
  const refresh = () => region().getByRole('button',{name:'刷新修订审阅',exact:true});
  const inspectButton = () => region().getByRole('button',{name:'查看完整原修订审阅',exact:true});
  const ready = async () => { await region().getByText('原项目修订候选',{exact:true}).waitFor(); await page.waitForFunction(()=>!Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='刷新修订审阅')?.disabled); };
  const set = async extra => { await page.evaluate(value=>window.set(value),{rows:[original],revision:'review-v1',...extra}); await page.waitForFunction(()=>document.querySelector('[data-epoch]')?.getAttribute('data-epoch')===String(window.epoch)); };
  const reset = async extra => { await set(extra); await page.getByRole('button',{name:'查看项目 p 的修订审阅',exact:true}).click(); await ready(); };
  const inspect = async () => { await inspectButton().click(); await detail().waitFor(); };
  const retained = async () => { assert.match(await region().innerText(),/ORIGINAL-REVISION/); assert.match(await detail().innerText(),/ORIGINAL-EVIDENCE/); assert.equal(await inspectButton().isDisabled(),true); };

  await set({direct:true,busy:true});
  await region().getByText('修订审阅尚未取得；这不表示没有可审阅候选。',{exact:true}).waitFor();
  assert.equal(await page.evaluate(()=>window.reads.length),0);
  await page.evaluate(()=>{window.busy=false;window.render()}); await ready();
  cases.push('workspace_revision_reviews_busy_initial_read_resumes');

  await reset();
  assert.deepEqual(await page.evaluate(()=>window.reads.at(-1)),{type:'revisionReviews',projectId:'p',offset:0,revision:null});
  assert.equal(await inspectButton().count(),1); await inspect();
  assert.deepEqual(await page.evaluate(()=>window.reads.at(-1)),{type:'revisionReview',projectId:'p',revision:'review-v1'});
  await detail().getByText('查看原记录字段',{exact:true}).click();
  assert.deepEqual(JSON.parse(await detail().locator('pre').innerText()),original);
  for(const text of ['已接受任务 1','已取消任务 2','已退役任务 3','待处理任务 4','已提交任务 5','已验证提交 6']) assert.ok((await detail().innerText()).includes(text));
  assert.equal(await region().getByRole('button',{name:/申请|退役|启动|暂停/}).count(),0);
  await page.screenshot({path:join(dir,'original-detail.png'),fullPage:true});
  cases.push('workspace_revision_reviews_first_read_detail_original_counts_digests_readonly');

  await reset(); await page.evaluate(()=>window.hold=true);
  await inspectButton().evaluate(button=>{button.click();button.click()}); await page.waitForFunction(()=>typeof window.finish==='function');
  assert.equal(await page.evaluate(()=>window.reads.filter(q=>q.type==='revisionReview').length),1);
  assert.equal(await page.evaluate(()=>window.maxActive),1); await page.evaluate(()=>window.finish()); await detail().waitFor();
  cases.push('workspace_revision_reviews_duplicate_lock');

  await reset(); await inspect(); await page.evaluate(()=>{window.busy=true;window.render()});
  await page.waitForFunction(()=>Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='刷新修订审阅')?.disabled);
  const busyReads = await page.evaluate(()=>window.reads.length);
  await refresh().evaluate(button=>button.click()); await inspectButton().evaluate(button=>button.click());
  assert.equal(await page.evaluate(()=>window.reads.length),busyReads);
  await page.evaluate(()=>{window.busy=false;window.render()});
  cases.push('workspace_revision_reviews_busy_blocks_followup_reads');

  for(const fail of ['stale workspace revision','storage unavailable','interrupted read','poisoned owner','nonshared owner']) {
    await reset(); await inspect(); await page.evaluate(value=>window.fail=value,fail); await refresh().click(); await region().getByRole('alert').waitFor(); await retained();
    const reads=await page.evaluate(()=>window.reads.length); await page.clock.runFor(5000); assert.equal(await page.evaluate(()=>window.reads.length),reads);
  }
  cases.push('workspace_revision_reviews_stale_storage_identity_errors_retain_page_detail_no_retry');

  const pagePatches = [{projectId:'foreign'},{revision:''},{offset:1},{total:2},{nextOffset:1},{entries:[]},{entries:[{...original,projectId:'foreign'}]},{entries:[{...original,acceptedTasks:-1}]},{entries:[{...original,evidenceDigest:'changed'}]},{unavailableReason:''},{unavailableReason:'busy'},{revision:'oversized',entries:[{...original,evidenceDigest:'x'.repeat(65536)}]}];
  for(const patch of pagePatches) {
    await reset(); await inspect(); await page.evaluate(value=>window.patch=value,patch); await refresh().click(); await region().getByRole('alert').waitFor(); await retained();
  }
  cases.push('workspace_revision_reviews_page_identity_revision_cursor_bounds_same_version_failures_retained');
  for(const patch of [{projectId:'foreign'},{revision:'foreign'},{record:{...original,projectId:'foreign'}},{record:{...original,revisionDigest:'changed'}},{record:{...original,verifiedSubmissions:7}},{record:null},{record:{...original,evidenceDigest:'x'.repeat(512*1024)}}]) {
    await reset(); await inspect(); await page.evaluate(value=>window.patch=value,patch); await inspectButton().click(); await region().getByRole('alert').waitFor(); await retained();
  }
  cases.push('workspace_revision_reviews_detail_identity_revision_counts_size_failures_retained');

  await reset(); await inspect(); await page.evaluate(()=>{window.revision='review-v2';window.unavailableReason='original busy guard: resource holding task'}); await refresh().click();
  await region().getByRole('alert').filter({hasText:'original busy guard'}).waitFor();
  assert.equal(await detail().count(),0); assert.equal(await inspectButton().count(),0); assert.doesNotMatch(await region().innerText(),/原项目当前没有可审阅修订候选/);
  cases.push('workspace_revision_reviews_unavailable_reason_not_empty_success');
  await set({rows:[]}); await page.getByRole('button',{name:'查看项目 p 的修订审阅',exact:true}).click(); await region().getByText('原项目当前没有可审阅修订候选。',{exact:true}).waitFor();
  await set({fail:'unknown project'}); await page.getByRole('button',{name:'查看项目 p 的修订审阅',exact:true}).click(); await region().getByRole('alert').waitFor(); assert.match(await region().innerText(),/尚未取得/); assert.doesNotMatch(await region().innerText(),/原项目当前没有可审阅修订候选/);
  cases.push('workspace_revision_reviews_known_empty_distinct_initial_error');

  await reset(); await inspect(); await page.evaluate(()=>window.hold=true); await inspectButton().click(); await page.waitForFunction(()=>typeof window.finish==='function'); await detail().getByRole('button',{name:'关闭',exact:true}).click(); await page.evaluate(()=>window.finish()); await page.clock.runFor(1); assert.equal(await detail().count(),0);
  cases.push('workspace_revision_reviews_closed_detail_late_response_ignored');
  for(const kind of ['page','detail']) {
    await reset(); await page.evaluate(()=>window.hold=true); await (kind==='page'?refresh():inspectButton()).click(); await page.waitForFunction(()=>typeof window.finish==='function');
    await page.getByRole('button',{name:'查看项目 other 的修订审阅',exact:true}).click(); await page.getByRole('heading',{name:'项目 other 的修订审阅',exact:true}).waitFor();
    await page.evaluate(()=>window.finish()); await page.clock.runFor(1); assert.doesNotMatch(await region().innerText(),/ORIGINAL-REVISION/); assert.equal(await detail().count(),0);
  }
  cases.push('workspace_revision_reviews_project_switch_late_page_detail_isolated');
  for(const kind of ['page','detail']) {
    await reset(); await page.evaluate(()=>window.hold=true); await (kind==='page'?refresh():inspectButton()).click(); await page.waitForFunction(()=>typeof window.finish==='function');
    await page.getByRole('button',{name:'关闭项目修订审阅',exact:true}).click(); await page.evaluate(()=>window.finish()); await page.clock.runFor(1); assert.equal(await region().count(),0);
  }
  cases.push('workspace_revision_reviews_unmount_late_page_detail_isolated');
  // Check every fixture reset as well as final state; the send stub records all mutations globally.
  mutations = await page.evaluate(()=>window.totalMutations ?? 0);
  assert.equal(await page.evaluate(()=>window.sends.length),0); assert.equal(mutations,0); assert.equal(networkRequests,0); assert.deepEqual(errors,[]);
  await writeFile(join(dir,'reads.json'),JSON.stringify(await page.evaluate(()=>window.allReads),null,2));
  const after=await hashes(); assert.deepEqual(after,before);
  await writeFile(join(dir,'result.json'),JSON.stringify({status:'passed',cases,sourceBefore:before,sourceAfter:after,pageErrors:errors,networkRequests,mutations,modelCalls:0,schema:{pageQuery:{type:'revisionReviews',projectId:'p',offset:0,revision:null},detailQuery:{type:'revisionReview',projectId:'p',revision:'review-v1'},pageReply:'{type,value:WorkspaceRevisionReviewPage}',detailReply:'{type,value:WorkspaceRevisionReviewDetail}',maxCandidates:1},scope:'Production WorkspaceBrowser and WorkspaceRevisionReviews with controlled offline reads; not installed Tauri'},null,2));
  console.log(JSON.stringify({status:'passed',cases:cases.length,artifact:dir}));
} catch(error) {
  await writeFile(join(dir,'result.json'),JSON.stringify({status:'failed',cases,error:String(error),pageErrors:errors,networkRequests,mutations},null,2)); throw error;
} finally { await browser?.close(); }
