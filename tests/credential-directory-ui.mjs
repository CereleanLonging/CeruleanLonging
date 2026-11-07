import assert from 'node:assert/strict';
import {mkdtemp,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {build} from 'vite';
import {chromium} from 'playwright-core';

assert(process.env.CERULEAN_CACHE_SESSION,'Use managed cache runner');
const run=await mkdtemp(resolve('reports/credential-directory-ui-'));
const bundle=join(process.env.CERULEAN_CACHE_SESSION,'credential-directory-ui');
const entry=join(run,'entry.tsx');
const data={dataDirectory:'synthetic-readonly',workflows:['one','two'].map(id=>({project:{id,root:'D:/fixture/'+id},state:{tasks:{}},approval:null})),projectSetups:[],workerAccountSetups:[],validators:[{id:'checks',name:'Fixture'}],managementDecisions:[],managementHistory:{projects:[]},openaiConnection:{models:[],keyConfigured:false},codexConnection:{models:[],executable:'D:/fixture/codex.exe'},providerConnections:[],projectRuns:[],codexJobs:[],apiJobs:[],adapters:[]};
const draft={projectId:'one',requirement:'Synthetic requirement',workerAdapter:'deepseek',codexExecutable:'',workerModel:'worker',apiWorker:{target:{kind:'provider',endpoint:{kind:'deepSeek'}},model:'worker',fileTools:['read-files','write-files'],maxRounds:3,maxToolCalls:16,maxOutputTokens:4096,maxTotalTokens:30000,requestTimeoutSeconds:120},chiefModel:'chief',deputyModel:'deputy',validatorId:'checks',checkArgs:[],artifactPaths:['result.txt'],workerMaxRequests:3,managementMaxRequests:10,maxSeconds:180};
const saved={digest:'saved-configuration',setup:{...draft,chiefDirectory:'D:/fixture/bound-chief',version:1,check:{id:'project-check',args:[],timeoutSeconds:60},policy:{maxRequests:3,maxSeconds:180}}};
await writeFile(entry,`import React from 'react';import{createRoot}from'react-dom/client';import{flushSync}from'react-dom';
import{ProjectPlanning}from'../../apps/desktop/src/ProjectPlanning';import{ProviderCard,providers}from'../../apps/desktop/src/ProviderConnections';import{saveAccessDraft}from'../../apps/desktop/src/accessDraft';
const root=createRoot(document.getElementById('root'));let generation=0;window.commands=[];window.store=saveAccessDraft;
window.showPlanning=(data,mode='all')=>flushSync(()=>root.render(<ProjectPlanning key={++generation} data={data} mode={mode} busy={false} send={async command=>{window.commands.push(command);return true;}}/>));
window.providerStatus={provider:'deepseek',keyConfigured:true,state:'listed',target:{kind:'deepSeek'},models:['fixture-model'],error:null};
const assignments=[{id:'worker',label:'执行端',target:{kind:'provider',endpoint:{kind:'deepSeek'}},model:'fixture-model',onSelect:()=>{}}];
window.showCard=()=>flushSync(()=>root.render(<ProviderCard entry={providers[0]} status={window.providerStatus} view="all" busy={false} connected={true} modelAssignments={assignments} send={async(command)=>{window.commands.push(command);if(command.type==='setProviderKey'){window.providerStatus={...window.providerStatus,state:'unchecked',models:[],error:null};window.showCard();}return true;}}/>));`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:bundle,emptyOutDir:false,lib:{entry,name:'CredentialDirectory',formats:['iife'],fileName:()=> 'ui.js'}}});
const browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
const page=await browser.newPage({viewport:{width:1280,height:1000}});
const cases=[],errors=[];let externalRequests=0;
try{
  page.setDefaultTimeout(10000);page.on('pageerror',error=>errors.push(error.message));
  await page.route('**/*',route=>route.request().url()==='https://cerulean.test/'?route.fulfill({contentType:'text/html',body:'<html lang="zh-CN"><meta charset="utf-8"><div id="root"></div></html>'}):(externalRequests++,route.abort()));
  await page.goto('https://cerulean.test/');await page.addStyleTag({path:'apps/desktop/src/style.css'});await page.addScriptTag({path:join(bundle,'ui.js')});
  const directory=page.locator('#planning-chief-directory');
  await page.evaluate(data=>window.showPlanning(data),data);
  await page.waitForFunction(()=>document.querySelector('#planning-chief-directory')?.value==='D:/fixture/one-chief');
  assert(await directory.evaluate(input=>input.readOnly));
  await directory.focus();await page.keyboard.press('Control+A');await page.keyboard.type('D:/unexpected');
  assert.equal(await directory.inputValue(),'D:/fixture/one-chief');
  await page.locator('#planning-project').selectOption('two');
  await page.waitForFunction(()=>document.querySelector('#planning-chief-directory').value==='D:/fixture/two-chief');
  cases.push('New projects have an uneditable derived directory; switching projects binds the correct path');

  await page.evaluate(({data,draft})=>{window.store(data.dataDirectory,'D:/fixture/one',null,{...draft,chiefDirectory:'D:/stale-access-chief'},true);window.showPlanning(data,'connections');},{data,draft});
  await page.waitForFunction(()=>document.querySelector('#planning-chief-directory').value==='D:/fixture/one-chief');
  assert(await directory.evaluate(input=>input.readOnly));
  cases.push('Persisted access drafts cannot restore an obsolete chief path');

  const configured={...data,projectSetups:[saved]};
  await page.evaluate(data=>window.showPlanning(data),configured);
  await page.waitForFunction(()=>document.querySelector('#planning-chief-directory').value==='D:/fixture/bound-chief');
  assert(await directory.evaluate(input=>input.readOnly));
  await page.evaluate(({data,draft})=>localStorage.setItem(JSON.stringify(['cerulean-next-execution',data.dataDirectory,'one']),JSON.stringify({draft:{...draft,chiefDirectory:'D:/stale-next-chief',worktree:{poolRoot:'D:/stale-pool'}},artifacts:'result.txt',stages:[{id:'project-check',name:'Fixture',validatorId:'checks',args:[],timeoutSeconds:60}]})),{data,draft});
  await page.getByRole('button',{name:'载入下一版草稿',exact:true}).click();
  await page.getByText('已载入下一版草稿，主指挥目录保持项目绑定；请重新审阅后保存配置。',{exact:true}).waitFor();
  assert.equal(await directory.inputValue(),'D:/fixture/bound-chief');
  await page.getByRole('button',{name:'保存需求与执行配置',exact:true}).click();
  await page.waitForFunction(()=>window.commands.some(c=>c.type==='configureProject'));
  const sent=await page.evaluate(()=>window.commands.find(c=>c.type==='configureProject').draft);
  assert.equal(sent.chiefDirectory,'D:/fixture/bound-chief');assert.equal(sent.worktree,undefined);
  cases.push('Existing project binding survives next-draft restore and submission, without a conflicting directory pool');
  await directory.locator('..').screenshot({path:join(run,'readonly-directory.png')});

  await page.evaluate(data=>window.showPlanning(data),{...data,projectSetups:[{...saved,digest:'legacy',setup:{...saved.setup,chiefDirectory:undefined}}]});
  await page.waitForFunction(()=>document.querySelector('#planning-chief-directory').value==='D:/fixture/one-chief');
  await page.getByRole('status').filter({hasText:'当前已保存配置尚未绑定独立目录'}).waitFor();
  cases.push('Legacy unbound configuration receives a proposed directory and explicit review/save notice');

  await page.evaluate(()=>{window.commands=[];window.showCard();});
  const test=page.getByRole('button',{name:'测试连通',exact:true});
  await page.getByRole('status').filter({hasText:'测试通过'}).waitFor();
  await page.getByLabel('DeepSeek API Key',{exact:true}).fill('sk-synthetic-replacement-only');
  assert(await test.isDisabled());
  assert(await page.getByRole('button',{name:'选择或修改模型',exact:true}).isDisabled());
  assert.equal(await page.getByRole('status').filter({hasText:'测试通过'}).count(),0);
  await page.getByRole('status').filter({hasText:'输入的新 Key 尚未保存'}).waitFor();
  await test.evaluate(button=>button.click());assert.deepEqual(await page.evaluate(()=>window.commands),[]);
  cases.push('Unsaved replacement blocks old-key testing and model selection and hides prior success');
  await page.getByRole('button',{name:'加密保存 DeepSeek Key',exact:true}).click();
  // A successful save opens the model picker. Close it before using the card.
  const later=page.getByRole('button',{name:'稍后选择',exact:true});
  await later.waitFor();await later.click();
  assert.equal(await page.getByLabel('DeepSeek API Key',{exact:true}).inputValue(),'');
  assert.equal(await test.isDisabled(),false);
  await test.click();
  assert.deepEqual((await page.evaluate(()=>window.commands)).map(c=>c.type),['setProviderKey','checkProviderModels']);
  const popup=page.getByRole('button',{name:'稍后选择',exact:true});if(await popup.isVisible())await popup.click();
  cases.push('Saving clears the input; the next explicit test uses the fixed DeepSeek endpoint');
  await page.evaluate(()=>{window.providerStatus={...window.providerStatus,state:'failed',error:'模型目录检查失败（HTTP 401）：DeepSeek 官方拒绝了已保存的 Key。请在官方开放平台确认 Key 是否有效；必要时新建 Key，在本卡片加密保存后重新测试'};window.showCard();});
  const alert=page.getByRole('alert');await alert.filter({hasText:'HTTP 401'}).waitFor();
  assert((await alert.innerText()).includes('DeepSeek 官方拒绝了已保存的 Key'));
  assert(!(await alert.innerText()).includes('地域'));
  assert.equal(await alert.getByRole('button',{name:'技术详情',exact:true}).count(),0);
  await page.screenshot({path:join(run,'credential-error.png')});
  cases.push('HTTP 401 and actionable Chinese guidance are visible without opening technical details');
  assert.deepEqual(errors,[]);assert.equal(externalRequests,0);
  await writeFile(join(run,'result.json'),JSON.stringify({passed:true,cases,errors,externalRequests,scope:'Production React with synthetic snapshots and recording IPC'},null,2));
  console.log(JSON.stringify({passed:true,cases:cases.length,run}));
}catch(error){await page.screenshot({path:join(run,'failure.png')}).catch(()=>{});await writeFile(join(run,'failure.json'),JSON.stringify({error:String(error),cases,errors},null,2));throw error;}
finally{await browser.close();}
