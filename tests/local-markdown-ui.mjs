// Production React components and local tool; all IPC is recorded, providers are forbidden.
import assert from 'node:assert/strict';
import {mkdtemp,writeFile,readFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {build} from 'vite';
import {chromium} from 'playwright-core';
assert(process.env.CERULEAN_CACHE_SESSION);
const run=await mkdtemp(resolve('reports/local-markdown-ui-'));
const bundle=join(process.env.CERULEAN_CACHE_SESSION,'local-markdown-ui');
const snapshot={dataDirectory:'synthetic-local-markdown',workflows:[{project:{id:'project',root:'D:/synthetic/project'},state:{tasks:{}},approval:null}],projectSetups:[],workerAccountSetups:[],validators:[],managementDecisions:[],managementHistory:{projects:[]},openaiConnection:{models:[],keyConfigured:false},codexConnection:{models:[],executable:''},providerConnections:[],projectRuns:[],codexJobs:[],apiJobs:[],adapters:[],desktopBindings:[]};
const entry=join(run,'entry.tsx');
await writeFile(entry,`import React,{useState}from'react';import{createRoot}from'react-dom/client';import{flushSync}from'react-dom';import{MarkdownImport,readMarkdown}from'../../apps/desktop/src/MarkdownImport';import{WorkspaceBrowser}from'../../apps/desktop/src/WorkspaceBrowser';import{ProjectPlanning}from'../../apps/desktop/src/ProjectPlanning';import{saveAccessDraft}from'../../apps/desktop/src/accessDraft';
const root=createRoot(document.getElementById('root'));let epoch=0;window.readMarkdown=readMarkdown;
const original=File.prototype.arrayBuffer;File.prototype.arrayBuffer=function(){if(window.holdRead){const file=this;return new Promise(resolve=>window.releaseRead=async()=>resolve(await original.call(file)));}return original.call(this);};
window.__TAURI_INTERNALS__={invoke:async(name,args)=>{window.native.push({name,args});if(name==='review_workflow_template'){JSON.parse(args.input);return{requirement:'工作流整理后的需求：逐步实现并核验 CSV。',template:{name:'CSV 工作流',workflowType:'coding',steps:[{id:'one'}],supervision:{repeatedFailureThreshold:2,maxAdvicePerAttempt:1}}};}throw Error('Forbidden native call: '+name);}};
function Import(){const[text,setText]=useState('原始草稿'),[disabled,setDisabled]=useState(false);window.setDisabled=setDisabled;return <><textarea aria-label="已有草稿" value={text} onChange={e=>setText(e.target.value)}/><MarkdownImport mode="requirement" disabled={disabled} contextKey={text} onApply={text=>{window.applied.push(text);setText(text);}}/></>;}
const read=async q=>q.type==='overview'?{type:'overview',value:{mode:'ready',legacyPlanCount:0,pendingLegacyPlanCount:0,projectCount:0}}:{type:q.type,value:{revision:'fixture',offset:0,nextOffset:null,total:0,entries:[]}};
window.show=mode=>{window.native=[];window.commands=[];window.applied=[];window.holdRead=false;window.releaseRead=undefined;epoch++;const data={...${JSON.stringify(snapshot)},dataDirectory:'synthetic-local-markdown-'+epoch};if(mode==='project')saveAccessDraft(data.dataDirectory,'D:/synthetic/project',null,{projectId:'project',workerAdapter:'codex',codexExecutable:'',workerModel:'',chiefModel:'external-codex-session',deputyModel:'deputy',managementSources:{chief:{kind:'codexSession',sessionId:'bound-local-chief'},deputy:{kind:'openAi'}},validatorId:'checks',checkArgs:[],artifactPaths:[],workerMaxRequests:3,managementMaxRequests:10,maxSeconds:180},true);flushSync(()=>root.render(mode==='project'?<ProjectPlanning key={epoch} data={data} mode="planning" busy={false} send={async c=>{window.commands.push(c);return true;}}/>:mode==='workspace'?<WorkspaceBrowser key={epoch} read={read} send={async c=>{window.commands.push(c);return true;}} enabled busy={false} showPlans onOverview={()=>{}}/>:<Import key={epoch}/>));};`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:bundle,emptyOutDir:false,lib:{entry,name:'LocalMarkdown',formats:['iife'],fileName:()=> 'ui.js'}}});
const browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
const cases=[],errors=[];let network=0;
const page=await browser.newPage({viewport:{width:1200,height:1000}});page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push(e.message));
const upload=async(name,text,locator=page.locator('input[type=file][accept=".md"]'))=>locator.setInputFiles({name,mimeType:'text/plain',buffer:Buffer.from(text)});
const confirm=()=>page.getByRole('button',{name:'确认读入草稿',exact:true});
const source='# CSV 工具\n## 目标\n支持中文 CSV。\n## 任务\n- 保留 5 个工作目录。\n## 验收标准\n- 367 项检查全部通过。\n## 约束\n不得上传文件。\n## 示例\n<img src="https://invalid.example/x" onerror="window.injected=true">';
try {
 const css=await readFile('apps/desktop/src/style.css','utf8');
 await page.route('**/*',route=>route.request().url()==='https://cerulean.test/'?route.fulfill({contentType:'text/html',body:`<html lang="zh-CN"><meta charset="utf-8"><style>${css}body{padding:24px}#root{max-width:1000px;margin:auto}</style><div id="root"></div></html>`}):(network++,route.abort()));
 await page.goto('https://cerulean.test/');await page.addScriptTag({path:join(bundle,'ui.js')});await page.evaluate(()=>window.show('import'));
 assert.equal(await page.locator('input[type=file]').isEnabled(),true);
 await upload('wrong.txt','text');await page.getByRole('alert').filter({hasText:'仅支持 .md'}).waitFor();
 await upload('empty.md',' \n');await page.getByRole('alert').filter({hasText:'为空'}).waitFor();
 await page.locator('input[type=file]').setInputFiles({name:'binary.md',mimeType:'text/plain',buffer:Buffer.from([0xff,0xfe,0x61])});await page.getByRole('alert').filter({hasText:'UTF-8'}).waitFor();
 assert.deepEqual(await page.evaluate(()=>window.native),[]);cases.push('No chief/Key prerequisite; unsupported, empty and non-UTF8 files fail locally');

 await upload('需求.MD',source);await confirm().waitFor();assert.equal(await page.getByLabel('已有草稿').inputValue(),'原始草稿');
 const proposed=await page.getByLabel('提炼内容，可修改',{exact:true}).inputValue();for(const text of ['目标','验收标准','367','不得上传'])assert(proposed.includes(text));
 await page.getByRole('button',{name:'查看原文与引用',exact:true}).click();assert.equal(await page.getByLabel('原文',{exact:true}).inputValue(),source);assert((await page.getByLabel('提炼依据与原文行号',{exact:true}).inputValue()).includes('第 1–1 行'));
 assert.equal(await page.locator('img').count(),0);assert.equal(await page.evaluate(()=>window.injected),undefined);assert.deepEqual(await page.evaluate(()=>window.native),[]);
 await page.getByLabel('提炼内容，可修改',{exact:true}).fill('审阅修改后的需求');await confirm().click();assert.equal(await page.getByLabel('已有草稿').inputValue(),'审阅修改后的需求');
 cases.push('Local classification, verbatim source and line references require editable explicit confirmation; source HTML never executes');

 await upload('cancel.md',source);await confirm().waitFor();await page.getByRole('button',{name:'取消导入',exact:true}).click();assert.equal(await page.getByLabel('已有草稿').inputValue(),'审阅修改后的需求');
 assert.equal(await confirm().count(),0);cases.push('Cancel leaves the existing draft unchanged');
 for(const change of ['draft','disabled']){
  await page.evaluate(()=>{window.show('import');window.holdRead=true;});await upload('late.md',source);await page.waitForFunction(()=>!!window.releaseRead);
  if(change==='draft')await page.getByLabel('已有草稿').fill('用户后来输入');else await page.evaluate(()=>window.setDisabled(true));
  await page.evaluate(()=>window.releaseRead());await page.waitForTimeout(60);assert.equal(await confirm().count(),0);assert.equal(await page.evaluate(()=>window.applied.length),0);
 }cases.push('Late file reads cannot overwrite edited context or a disabled form');
 await page.evaluate(()=>{window.show('import');window.holdRead=true;});await upload('stop.md',source);await page.waitForFunction(()=>!!window.releaseRead);await page.getByRole('button',{name:'停止本地提炼'}).click();await page.evaluate(()=>window.releaseRead());await page.waitForTimeout(60);assert.equal(await confirm().count(),0);cases.push('Stopping local reading discards late output without background requests');

 await page.evaluate(()=>window.show('import'));const large='# 全文\n'+Array.from({length:1500},(_,i)=>'- 条目 '+i+'：保留中文😀和验收数据。').join('\n');await upload('large.md',large);await confirm().waitFor();assert((await page.getByLabel('提炼内容，可修改',{exact:true}).inputValue()).includes('条目 1499'));await page.getByRole('button',{name:'查看原文与引用'}).click();assert.equal(await page.getByLabel('原文',{exact:true}).inputValue(),large);cases.push('Large Unicode document above 64KiB is fully preserved without a result-item cap');


 const streamed=await page.evaluate(async()=>{
  const source='x'.repeat(262142)+'😀\r\n中文末尾';const updates=[];
  const text=await window.readMarkdown(new File([source],'split.md'),()=>true,(done,total)=>updates.push([done,total]));
  return {same:text===source.replace(/\r\n/g,'\n'),updates};
 });assert(streamed.same);assert(streamed.updates.length>1);cases.push('UTF-8 emoji across 256KiB reads and CRLF remain exact with actual byte progress');
 await page.evaluate(()=>window.show('import'));
 const segmented='# 超长文档\n## 任务\n'+Array.from({length:12000},(_,i)=>'- 第 '+i+' 项：保留中文与数字123，限制不能删除。').join('\n');
 await upload('segmented.md',segmented);await confirm().waitFor();
 const merged=await page.getByLabel('提炼内容，可修改',{exact:true}).inputValue();assert(merged.includes('第 0 项'));assert(merged.includes('第 11999 项'));
 await page.getByRole('button',{name:'查看原文与引用',exact:true}).click();
 let original='';
 while(true){original+=await page.getByLabel('原文',{exact:true}).inputValue();const next=page.getByRole('button',{name:'下一段原文',exact:true});if(await next.isDisabled())break;await next.click();}
 assert.equal(original,segmented);assert((await page.getByLabel('提炼依据与原文行号',{exact:true}).inputValue()).includes('第 1–1 行'));
 await page.getByRole('button',{name:'下一段依据',exact:true}).click();assert(!(await page.getByLabel('提炼依据与原文行号',{exact:true}).inputValue()).includes('第 1–1 行'));
 await confirm().click();assert.equal(await page.getByLabel('已有草稿').inputValue(),merged);cases.push('A multi-segment document merges all requirements, pages original evidence and enters the textbox only after confirmation');
 await page.evaluate(()=>window.show('workspace'));await page.waitForFunction(()=>!document.querySelector('input[type=file]')?.disabled);assert.equal(await page.locator('#document-refinement-project').count(),0);await upload('plan.md','# 本地计划\n## 任务\n- 实现 CSV\n## 验收\n- 保留全部数据');await confirm().click();assert.equal(await page.evaluate(()=>window.commands.length),0);await page.getByRole('button',{name:'保存计划',exact:true}).click();await page.waitForFunction(()=>window.commands.length===1);assert.equal(await page.evaluate(()=>window.commands[0].type),'createPlan');assert((await page.evaluate(()=>window.commands[0].tasks)).some(t=>t.includes('保留全部数据')));cases.push('Manual plans import locally without project/model configuration and still require separate Save');

 await page.evaluate(()=>window.show('project'));await page.getByText('Codex 主指挥会话：bound-local-chief',{exact:true}).waitFor();
 const composer=page.locator('.requirement-composer'),md=page.getByRole('button',{name:'导入 Markdown 文档',exact:true}),workflow=page.getByRole('button',{name:'导入本地工作流',exact:true}),requirement=page.locator('#planning-requirement');
 assert.equal(await composer.count(),1);assert.equal(await composer.locator('input[type=file]:visible').count(),0);assert.equal(await composer.getByRole('button').count(),3);assert(await composer.getByRole('button',{name:'打开拖拽导入窗口',exact:true}).isEnabled());assert(await md.isEnabled());assert(await workflow.isEnabled());
 await requirement.fill('保留现有需求');const chooserPromise=page.waitForEvent('filechooser');await md.click();const chooser=await chooserPromise;await chooser.setFiles({name:'attached.md',mimeType:'text/markdown',buffer:Buffer.from(source)});await page.getByRole('dialog',{name:'Markdown 本地提炼',exact:true}).waitFor();await confirm().waitFor();
 assert.equal(await requirement.inputValue(),'保留现有需求');assert.equal(await page.getByRole('dialog').evaluate(e=>e.matches(':modal')),true);await page.keyboard.press('Escape');assert.equal(await page.getByRole('dialog').count(),0);assert.equal(await requirement.inputValue(),'保留现有需求');assert.equal(await md.evaluate(e=>e===document.activeElement),true);
 cases.push('Single composer with lower-left plus/paperclip; bound Codex can import, modal preview cancels safely and restores focus');
 await upload('attached.md',source,composer.locator('input[accept=".md"]'));await confirm().waitFor();await confirm().click();assert((await requirement.inputValue()).includes('367'));assert.equal(await page.evaluate(()=>window.commands.length),0);assert.deepEqual(await page.evaluate(()=>window.native),[]);cases.push('Paperclip imports through the local tool without any model/IPC request or automatic project submission');

 const workflowChooser=page.waitForEvent('filechooser');await workflow.click();await (await workflowChooser).setFiles({name:'flow.json',mimeType:'application/json',buffer:Buffer.from('{"schemaVersion":1}')});await page.getByRole('dialog',{name:'导入本地工作流',exact:true}).waitFor();await page.getByRole('button',{name:'使用此工作流需求',exact:true}).waitFor();
 assert.equal(await page.getByRole('dialog',{name:'导入本地工作流',exact:true}).getByRole('checkbox').isChecked(),false);assert.equal(await page.evaluate(()=>window.native.length),1);assert.equal(await page.evaluate(()=>window.native[0].name),'review_workflow_template');await page.getByRole('button',{name:'使用此工作流需求',exact:true}).click();assert.equal(await requirement.inputValue(),'工作流整理后的需求：逐步实现并核验 CSV。');assert.equal(await page.evaluate(()=>window.commands.length),0);cases.push('Plus preserves existing JSON validation and opt-in supervision; applying preview only edits the requirement draft');
 for(const width of [1200,640,430]){await page.setViewportSize({width,height:1000});const boxes=await Promise.all([workflow.boundingBox(),md.boundingBox(),requirement.boundingBox()]);assert.equal(boxes[0].y,boxes[1].y);assert(boxes[0].x<boxes[1].x);assert(boxes[0].y>=boxes[2].y+boxes[2].height);assert.equal(await composer.evaluate(e=>e.scrollWidth<=e.clientWidth+1),true);await composer.screenshot({path:join(run,`composer-${width}.png`)});}
 await upload('preview.md',source,composer.locator('input[accept=".md"]'));await confirm().waitFor();await page.getByRole('dialog').screenshot({path:join(run,'local-preview-430.png')});assert.equal(await page.getByRole('dialog').evaluate(e=>e.scrollWidth<=e.clientWidth+1),true);cases.push('Toolbar and preview remain usable at desktop, medium and mobile widths without horizontal overflow');
 assert.equal(network,0);assert.deepEqual(errors,[]);await writeFile(join(run,'result.json'),JSON.stringify({passed:true,cases,errors,networkRequests:network,realModelCalls:0,scope:'Real local tool and React; recorded workflow IPC only'},null,2));console.log(JSON.stringify({passed:true,cases:cases.length,report:run}));
}catch(error){await writeFile(join(run,'failure.json'),JSON.stringify({error:String(error.stack),cases,errors,network},null,2));await page.screenshot({path:join(run,'failure.png'),fullPage:true}).catch(()=>{});throw error;}finally{await browser.close();}
