import {mkdtemp, readFile, writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import assert from 'node:assert/strict';
import {build} from 'vite';
import {chromium} from 'playwright-core';
if (process.env.CERULEAN_NATIVE_OWNED_JOB !== '1') throw Error('Use owned runner');
const dir = await mkdtemp('reports/workspace-configuration-ui-');
const cases = [], errors = [];
let browser, page, networkRequests = 0;
const entry = join(dir, 'entry.tsx');
await writeFile(entry, `
import React from 'react';
import {createRoot} from 'react-dom/client';
import {WorkspaceRecordsHarness as WorkspaceBrowser} from '../../tests/fixtures/WorkspaceRecordsHarness';
const root = createRoot(document.getElementById('root'));
window.epoch = 0; window.tick = 0; window.allReads = []; window.actions = 0;
window.render = () => {
  window.tick++;
  root.render(<React.StrictMode><div data-tick={window.tick}>
    <WorkspaceBrowser key={window.epoch} enabled={true} busy={false} showPlans={false}
      send={async () => { window.actions++; throw Error('unexpected command'); }}
      onOverview={() => {}}
      read={async q => {
        window.reads.push(q); window.allReads.push(q);
        if(q.type === 'overview') return {type:'overview',value:{product:'local',protocolVersion:1,dataDirectory:'controlled',mode:'ready',legacyPlanCount:0,pendingLegacyPlanCount:0,projectCount:0,message:null}};
        if(q.type === 'plans' || q.type === 'projects') return {type:q.type,value:{revision:'base',offset:0,total:0,nextOffset:null,entries:[]}};
        if(q.type !== 'configurations' && q.type !== 'configuration') throw Error('unexpected read '+q.type);
        if(window.fail) throw Error('controlled read unavailable');
        let value = structuredClone(q.type === 'configurations' ? window.pages[q.offset] : window.originalDetail);
        if(window.fault === 'revision') value.revision = 'expired';
        if(window.fault === 'cursor') value.offset = 999;
        if(window.fault === 'project') value.projectId = 'foreign';
        if(window.fault === 'setupProject') value.saved.setup.projectId = 'foreign';
        if(window.fault === 'digest') value.saved.digest = 'wrong';
        if(window.hold === q.type) return await new Promise(resolve => { window.finish = () => {resolve({type:q.type,value});window.finish=null;}; });
        return {type:q.type,value};
      }}/>
  </div></React.StrictMode>);
};
window.set = state => { window.epoch++; Object.assign(window,{reads:[],fail:false,fault:null,hold:null,finish:null},state); window.render(); };
`);
const setup = {projectId:'alpha',version:7,requirement:'REQUIREMENT ORIGINAL',worker:null,chiefModel:'chief-controlled',deputyModel:'deputy-controlled',validatorId:'validator-controlled',check:{command:'npm test',timeoutSeconds:30},acceptance:null,artifactPaths:['artifacts/a'],policy:{allowNetwork:false},resourceKeys:['key-a'],profiles:[],worktree:null,desktopWorker:null,apiWorker:null,workerAdapter:'local'};
const state = {originalDetail:{projectId:'alpha',revision:'config-v1',saved:{setup,digest:'digest-alpha'}},pages:{0:{revision:'config-v1',offset:0,total:2,nextOffset:5,entries:[{projectId:'alpha',digest:'digest-alpha'}]},5:{revision:'config-v1',offset:5,total:2,nextOffset:null,entries:[{projectId:'beta',digest:'digest-beta'}]}}};
try {
  await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:join(dir,'bundle'),emptyOutDir:false,lib:{entry,name:'ConfigurationUI',formats:['iife'],fileName:()=> 'ui.js'}}});
  browser = await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
  page = await browser.newPage(); page.setDefaultTimeout(10000);
  page.on('pageerror',e=>errors.push(e.message));
  await page.route('**/*',r=>{networkRequests++; return r.abort();});
  await page.setContent('<html lang="zh-CN"><div id="root"></div></html>');
  await page.addScriptTag({content:await readFile(join(dir,'bundle/ui.js'),'utf8')});
  const region=()=>page.getByRole('region',{name:'已保存项目配置',exact:true});
  const detail=()=>page.getByRole('region',{name:'原项目配置详情',exact:true});
  const nav=()=>page.getByRole('navigation',{name:'配置分页',exact:true});
  const refresh=()=>region().getByRole('button',{name:'刷新配置首页',exact:true});
  const original=()=>region().getByRole('button',{name:'查看项目 alpha 的原配置',exact:true});
  const close=()=>page.getByRole('button',{name:'关闭已保存项目配置',exact:true});
  const reset=async extra=>{
    await page.evaluate(s=>window.set(s),{...state,...extra});
    await page.waitForFunction(()=>document.querySelector('[data-tick]')?.getAttribute('data-tick')===String(window.tick));
    await page.getByRole('button',{name:'查看已保存项目配置',exact:true}).click();
    await region().waitFor();
    if (!extra?.hold) await page.waitForFunction(()=>!Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='刷新配置首页')?.disabled);
  };
  const showOriginal=async()=>{await original().click();await detail().waitFor();};
  await reset();
  assert.deepEqual(await page.evaluate(()=>window.reads.filter(q=>q.type==='configurations')),[{type:'configurations',offset:0,revision:null}]);
  assert.equal(await page.evaluate(()=>window.allReads.some(q=>q.type==='snapshot')),false);
  assert.equal(await page.evaluate(()=>window.actions),0);
  cases.push('opening production configuration entry reads bounded list without snapshot or commands');
  await nav().getByRole('button',{name:'下一页'}).click();
  await region().getByRole('heading',{name:'项目 beta',exact:true}).waitFor();
  assert.deepEqual(await page.evaluate(()=>window.reads.at(-1)),{type:'configurations',offset:5,revision:'config-v1'});
  await nav().getByRole('button',{name:'上一页'}).click();
  await original().waitFor();
  assert.deepEqual(await page.evaluate(()=>window.reads.at(-1)),{type:'configurations',offset:0,revision:'config-v1'});
  cases.push('server cursor 5 and back history bind offset and collection revision');
  await showOriginal();
  assert.deepEqual(JSON.parse(await detail().locator('pre').innerText()),setup);
  assert.equal(await detail().getByText('原配置摘要：digest-alpha',{exact:true}).count(),1);
  assert.equal(await detail().getByText('REQUIREMENT ORIGINAL',{exact:true}).count(),1);
  assert.equal(await detail().getByText('配置版本 7 · 主指挥 chief-controlled · 副指挥 deputy-controlled',{exact:true}).count(),1);
  cases.push('entire original saved setup and summary models requirement version digest are rendered');
  for(const fault of ['revision','cursor']) {
    await reset();await showOriginal();await page.evaluate(f=>window.fault=f,fault);
    await nav().getByRole('button',{name:'下一页'}).click();await region().getByRole('alert').waitFor();
    assert.equal(await detail().getByText('REQUIREMENT ORIGINAL',{exact:true}).count(),1);
    assert.equal(await original().isDisabled(),true);assert.equal(await nav().getByRole('button',{name:'下一页'}).isDisabled(),true);
  }
  cases.push('expired revision and wrong cursor independently retain prior list/detail and disable continuation');
  for(const fault of ['revision','project','setupProject','digest']) {
    await reset();await showOriginal();await page.evaluate(f=>window.fault=f,fault);
    await original().click();await region().getByRole('alert').waitFor();
    assert.deepEqual(JSON.parse(await detail().locator('pre').innerText()),setup);
    assert.equal(await original().isDisabled(),true);assert.equal(await nav().getByRole('button',{name:'下一页'}).isDisabled(),true);
  }
  cases.push('expired detail revision foreign envelope/setup project and summary digest mismatch preserve original independently');
  await reset({fail:true});await region().getByRole('alert').waitFor();
  assert.equal(await region().getByText('配置清单尚未取得。',{exact:true}).count(),1);
  assert.equal(await region().getByText('尚无已保存项目配置。',{exact:true}).count(),0);
  cases.push('unavailable first read remains unacquired not empty');
  await reset();await page.evaluate(()=>window.hold='configuration');
  await original().evaluate(b=>{b.click();b.click();});await page.waitForFunction(()=>typeof window.finish==='function');
  assert.equal(await page.evaluate(()=>window.reads.filter(q=>q.type==='configuration').length),1);
  await page.evaluate(()=>window.finish());await detail().waitFor();
  cases.push('synchronous double click issues one controlled detail read');
  await reset();await page.evaluate(()=>window.hold='configuration');await original().click();
  await page.waitForFunction(()=>typeof window.finish==='function');await close().click();await page.evaluate(()=>window.finish());
  await page.getByRole('button',{name:'查看已保存项目配置',exact:true}).click();await original().waitFor();
  assert.equal(await detail().count(),0);
  cases.push('late detail after close does not populate newly opened configuration panel');
  await reset({hold:'configurations'});await page.waitForFunction(()=>typeof window.finish==='function');
  await close().click();await page.evaluate(()=>{window.late=window.finish;window.hold=null;});
  await page.getByRole('button',{name:'查看已保存项目配置',exact:true}).click();await original().waitFor();
  await page.evaluate(()=>window.late());assert.equal(await detail().count(),0);
  assert.equal(await region().getByRole('heading',{name:'项目 alpha',exact:true}).count(),1);
  cases.push('late initial page after close is isolated from reopened configuration instance');
  assert.equal(await page.evaluate(()=>window.actions),0);assert.equal(await page.evaluate(()=>window.allReads.some(q=>q.type==='snapshot')),false);
  assert.equal(networkRequests,0);assert.deepEqual(errors,[]);
  await writeFile(join(dir,'result.json'),JSON.stringify({status:'passed',cases,pageErrors:errors,networkRequests,modelCalls:0,userDesktopActions:0,scope:'Controlled test-only record navigation host + WorkspaceConfigurations UI; not installed Tauri or installation integration',controlledReads:true},null,2));
  console.log(JSON.stringify({status:'passed',cases:cases.length,artifact:dir}));
} catch(e) {
  await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});
  await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(e),stack:e.stack,cases,errors},null,2));throw e;
} finally {await browser?.close();}
