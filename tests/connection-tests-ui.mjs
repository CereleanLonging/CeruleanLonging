import assert from 'node:assert/strict';
import {mkdtemp,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {build} from 'vite';
import {chromium} from 'playwright-core';
import {openConnectionMethod,deferModelChoice} from './connection-ui-helpers.mjs';

const directory = await mkdtemp(resolve('reports/connection-tests-ui-'));
assert(process.env.CERULEAN_CACHE_SESSION,'Use managed cache runner');
const bundle=join(process.env.CERULEAN_CACHE_SESSION,'connection-tests-ui');
const entry = join(directory,'entry.tsx');
await writeFile(entry, `import React from 'react';import {createRoot} from 'react-dom/client';import {flushSync} from 'react-dom';
import {ProjectPlanning} from '../../apps/desktop/src/ProjectPlanning';
import {IntegrationHub} from '../../apps/desktop/src/IntegrationHub';
import {LocalConnectionTest} from '../../apps/desktop/src/LocalConnectionTest';
const root=createRoot(document.getElementById('root'));let generation=0;
window.commands=[];window.nativeCalls=[];window.pending=[];
window.__TAURI_INTERNALS__={invoke:async(name,args)=>{window.nativeCalls.push({name,args});if(name==='test_local_connection')return new Promise((resolve,reject)=>window.pending.push({resolve,reject}));throw Error('Unexpected native command '+name);}};
const send=async command=>{window.commands.push(command);return true;};
window.showProject=data=>{window.fixture=data;flushSync(()=>root.render(<ProjectPlanning key={++generation} mode="connections" data={data} busy={false} send={send}/>));};
window.refreshProject=()=>flushSync(()=>root.render(<ProjectPlanning key={generation} mode="connections" data={{...window.fixture}} busy={false} send={send}/>));
window.showHub=data=>flushSync(()=>root.render(<IntegrationHub key={++generation} data={data} busy={false} send={send} configure={()=>{}}/>));
window.showLocal=request=>flushSync(()=>root.render(<LocalConnectionTest scope="fixture" request={request} busy={false}/>));`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:bundle,emptyOutDir:false,lib:{entry,name:'ConnectionTestsUI',formats:['iife'],fileName:()=> 'ui.js'}}});

const observation = {window:{app:'qoder',hwnd:'1',pid:12,startTicks:'123',exe:'D:/synthetic/qoder.exe',title:'Fixture',version:'0.4.3.0'},capturedAt:'2026-11-04T00:00:00Z',modelLabel:'fixture-model',sessionId:'12345678-1234-1234-1234-123456789abc',documentRuntimeId:'fixture-document',contentDigest:'a'.repeat(64),layoutDigest:'b'.repeat(64),nodeCount:10,draftEmpty:true,busy:false};
const binding = {id:'c'.repeat(64),projectId:'project',label:'Fixture Qoder',observation,confirmedAt:observation.capturedAt,bridgeSha256:'d'.repeat(64)};
const fixture = {dataDirectory:'connection-test-fixture',events:[],workflows:[{project:{id:'project',root:'D:/synthetic/project',timezone:'UTC'},state:{tasks:{}},tasks:[],approval:null}],projectSetups:[],workerAccountSetups:[],validators:[],managementDecisions:[],managementHistory:{projects:[]},openaiConnection:{state:'unchecked',models:[],keyConfigured:true,error:null},codexConnection:{state:'available',models:['fixture-model'],executable:'D:/synthetic/codex.exe',checkedAt:observation.capturedAt},providerConnections:['deepseek','anthropic','qwen','kimi','a6api','grok'].map(provider=>({provider,state:'unchecked',keyConfigured:true,models:[],target:null,error:null})),projectRuns:[],codexJobs:[],apiJobs:[],ompJobs:[],verificationJobs:[],desktopAccess:{bindings:[binding],windows:[],trials:[],checking:false,observation:null},adapters:[]};
let browser,page;
const cases=[],errors=[];
try {
  browser = await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
  page = await browser.newPage({viewport:{width:1280,height:1000}});
  page.on('pageerror',error=>errors.push(String(error)));
  await page.route('**/*',route=>route.fulfill({contentType:'text/html',body:'<html lang="zh-CN"><meta charset="utf-8"><div id="root"></div></html>'}));
  await page.goto('http://localhost/connection-test-fixture');
  await page.addStyleTag({path:resolve('apps/desktop/src/style.css')});
  await page.addScriptTag({path:join(bundle,'ui.js')});
  await page.evaluate(data=>window.showProject(data),fixture);
  const region = name => page.getByRole('region',{name,exact:true});
  const test = scope => scope.getByRole('button',{name:'测试连通',exact:true});
  const commands = () => page.evaluate(()=>window.commands);
  const nativeCalls = () => page.evaluate(()=>window.nativeCalls);

  await openConnectionMethod(page,'主指挥','api');
  await test(region('主指挥')).click();
  await deferModelChoice(page);
  assert.deepEqual((await commands()).at(-1),{type:'checkOpenAi'});
  await openConnectionMethod(page,'主指挥','key');
  await deferModelChoice(page);
  await test(region('主指挥')).click();
  await deferModelChoice(page);
  assert.deepEqual((await commands()).at(-1),{type:'checkOpenAi'});
  await openConnectionMethod(page,'副指挥','api');
  for (const [value,endpoint] of [['deepSeek',{kind:'deepSeek'}],['provider:anthropic',{kind:'anthropic'}],['provider:kimiChina',{kind:'kimiChina'}],['provider:grok',{kind:'grok'}],['provider:a6Api',{kind:'a6Api'}],['provider:a6ApiVip',{kind:'a6ApiVip'}],['provider:qwenSingapore',{kind:'qwenSingapore'}],['provider:qwenHongKong',{kind:'qwenHongKong'}],['provider:qwenBeijing',{kind:'qwenBeijing',workspaceId:'fixture-workspace'}]]) {
    await page.locator('#planning-deputy-source').selectOption(value);
    if(endpoint.kind==='qwenBeijing') {
      assert(await test(region('副指挥')).isDisabled());
      await page.locator('#planning-deputy-workspace').fill(endpoint.workspaceId);
    }
    await test(region('副指挥')).click();
    await deferModelChoice(page);
    assert.deepEqual((await commands()).at(-1),{type:'checkProviderModels',target:endpoint});
  }
  cases.push('API and Key buttons route OpenAI plus every supported provider and regional endpoint correctly');

  await page.evaluate(()=>{window.fixture.providerConnections.find(p=>p.provider==='qwen').target={kind:'qwenBeijing',workspaceId:'fixture-workspace'};window.fixture.providerConnections.find(p=>p.provider==='qwen').state='checking';window.refreshProject();});
  assert(await region('副指挥').getByRole('button',{name:'测试中…',exact:true}).isDisabled());
  await page.evaluate(()=>{const status=window.fixture.providerConnections.find(p=>p.provider==='qwen');status.state='failed';status.error='401 unauthorized';window.refreshProject();});
  await region('副指挥').getByRole('alert').filter({hasText:'身份验证或访问权限检查未通过'}).waitFor();
  await page.evaluate(()=>{const status=window.fixture.providerConnections.find(p=>p.provider==='qwen');status.state='listed';status.error=null;window.refreshProject();});
  await region('副指挥').getByRole('status').filter({hasText:'测试通过'}).waitFor();
  await page.locator('#planning-deputy-workspace').fill('changed-workspace');
  assert.equal(await region('副指挥').getByRole('status').filter({hasText:'测试通过'}).count(),0);
  cases.push('In-flight API checks disable repeats; failure and success are visible and stale endpoint success disappears');

  await openConnectionMethod(page,'主指挥','desktop');
  assert(await test(region('主指挥')).isDisabled());
  await page.locator('#planning-chief-session').fill('selected-session');
  await test(region('主指挥')).click();
  assert.deepEqual((await nativeCalls()).at(-1).args.request,{kind:'codexSession',sessionId:'selected-session',executable:'D:/synthetic/codex.exe'});
  assert(await region('主指挥').getByRole('button',{name:'测试中…',exact:true}).isDisabled());
  await page.locator('#planning-chief-session').fill('changed-session');
  await page.evaluate(()=>window.pending.shift().resolve({message:'old session success',checkedAt:new Date().toISOString()}));
  assert.equal(await page.getByText(/old session success/).count(),0);
  await test(region('主指挥')).click();
  await page.evaluate(()=>window.pending.shift().reject('找不到所填会话'));
  await region('主指挥').getByRole('alert').filter({hasText:'找不到所填会话'}).waitFor();
  await test(region('主指挥')).click();
  await page.evaluate(()=>window.pending.shift().resolve({message:'当前会话可读取',checkedAt:new Date().toISOString()}));
  await region('主指挥').getByRole('status').filter({hasText:'当前会话可读取'}).waitFor();
  cases.push('Codex desktop tests the entered session, ignores an older in-flight result after edits, and supports retry');

  assert.equal(await region('执行端').locator('[data-method="cli"]').count(),1);
  assert.equal(await region('执行端').getByRole('button',{name:'扫描本地 Codex',exact:true}).count(),0);
  await openConnectionMethod(page,'执行端','desktop');
  await region('执行端').getByRole('button',{name:'将桌面端接入设为默认执行端',exact:true}).click();
  await page.locator('#planning-adapter').selectOption('omp');
  assert(await test(region('执行端')).isDisabled());
  await page.locator('#planning-executable').fill('D:/synthetic/omp.exe');
  await page.locator('#planning-worker').fill('deepseek-flash');
  await page.locator('summary').filter({hasText:'角色目录与隔离设置'}).click();
  assert(await page.locator('#planning-chief-directory').evaluate(input=>input.readOnly));
  assert.equal(await page.locator('#planning-omp-runtime').count(),0);
  await test(region('执行端')).click();
  await page.getByRole('dialog',{name:'确认本次 OMP 接入'}).getByRole('button',{name:'确认并检查'}).click();
  assert.deepEqual((await nativeCalls()).at(-1).args.request,{kind:'omp',executable:'D:/synthetic/omp.exe',model:'deepseek-flash',runtimeParent:undefined,reasoningEffort:'max'});
  await page.evaluate(()=>window.pending.shift().resolve({message:'OMP 协议与认证通过',checkedAt:new Date().toISOString()}));
  await region('执行端').getByRole('status').filter({hasText:'OMP 协议与认证通过'}).waitFor();
  cases.push('CLI is optional; desktop OMP confirms and tests its exact executable, model, effort and runtime');

  const profile = region('执行配置 1');
  await region('执行端').getByRole('button',{name:'添加桌面端接入执行配置',exact:true}).click();
  await page.locator('#profile-0-adapter').selectOption('omp');
  await page.locator('#profile-0-exe').fill('D:/synthetic/extra/omp.exe');
  await page.locator('#profile-0-model').fill('extra-model');
  await test(profile).click();
  await page.getByRole('dialog',{name:'确认本次 OMP 接入'}).getByRole('button',{name:'确认并检查'}).click();
  assert.equal((await nativeCalls()).at(-1).args.request.model,'extra-model');
  await page.evaluate(()=>window.pending.shift().reject('所选模型不可用'));
  await profile.getByRole('alert').filter({hasText:'所选模型不可用'}).waitFor();
  cases.push('Each additional OMP profile has an independent test using its own configuration');

  await openConnectionMethod(page,'执行端','desktop');
  await page.locator('#planning-adapter').selectOption('qoder');
  const desktop = region('桌面编程会话配置');
  const bindDesktop = desktop.getByRole('button',{name:'测试并绑定会话',exact:true});
  assert(await bindDesktop.isDisabled());
  await page.locator('#planning-desktop-session-id').fill(observation.sessionId);
  await bindDesktop.click();
  assert.deepEqual((await commands()).at(-1),{type:'bindDesktopSession',projectId:'project',sessionId:observation.sessionId});
  await desktop.getByRole('status').filter({hasText:'会话已核对并绑定'}).waitFor();
  cases.push('Qoder binds the entered session ID and reads its verified model automatically');
  await page.screenshot({path:join(directory,'connections.png'),fullPage:true});

  await page.evaluate(data=>window.showHub({...data,workflows:[]}),fixture);
  for(const [name,method] of [['主指挥','api'],['主指挥','key'],['主指挥','desktop'],['副指挥','api'],['副指挥','key'],['执行端','key'],['执行端','desktop']]) {
    await openConnectionMethod(page,name,method);
    if (method === 'key') await deferModelChoice(page);
    const summary=region(name).locator(':scope > details[open]');
    if(name==='执行端'&&method==='desktop') await summary.getByText('关联项目后，填写 Qoder 会话 ID',{exact:false}).waitFor();
    else assert(await summary.getByRole('button',{name:'测试连通',exact:true}).count()>0);
  }
  assert.equal(await region('执行端').getByLabel('CLI 程序').count(),0);
  await region('执行端').getByLabel('OMP 程序完整路径').fill('D:/synthetic/omp.exe');
  assert.equal(await region('执行端').getByLabel('OMP 模型 ID').inputValue(),'deepseek-flash');
  cases.push('All unbound access groups also expose a test button with prerequisites');
  const action=page.locator('.connection-test:visible').first();
  const button=await action.locator(':scope > button').boundingBox();
  const feedback=await action.locator('.connection-test-feedback').boundingBox();
  assert(button.width>=112&&button.height>=42);
  assert(feedback.x>=button.x+button.width+19);
  await action.screenshot({path:join(directory,'test-layout-desktop.png')});
  await page.setViewportSize({width:390,height:844});
  const narrowButton=await action.locator(':scope > button').boundingBox();
  const narrowFeedback=await action.locator('.connection-test-feedback').boundingBox();
  assert(narrowFeedback.y>=narrowButton.y+narrowButton.height+11);
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  await action.screenshot({path:join(directory,'test-layout-narrow.png')});
  cases.push('Test controls have their own spaced action area; narrow windows stack feedback below a full-size button without overflow');
  assert.deepEqual(errors,[]);
  await writeFile(join(directory,'result.json'),JSON.stringify({status:'passed',cases,pageErrors:errors,scope:'Production React components and mocked native transport; no live provider calls'},null,2));
  console.log(JSON.stringify({status:'passed',cases:cases.length,directory}));
} catch(error) {
  await page?.screenshot({path:join(directory,'failure.png'),fullPage:true}).catch(()=>{});
  await writeFile(join(directory,'failure.json'),JSON.stringify({error:String(error),stack:error.stack,cases,errors},null,2));
  throw error;
} finally { await browser?.close(); }
