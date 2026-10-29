import {readFile,writeFile,mkdtemp} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
import {build} from 'vite';
import {chromium} from 'playwright-core';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/desktop-run-version-ui-'));
const entry=join(report,'entry.tsx');
await writeFile(entry,`import React from 'react';import {createRoot} from 'react-dom/client';import {ProjectRuns} from '../../apps/desktop/src/ProjectRuns';import {ProjectRuns as Previous} from '../desktop-supervision-195/before/apps/desktop/src/ProjectRuns';import {DesktopWorkerFields} from '../../apps/desktop/src/DesktopWorkerFields';const root=createRoot(document.getElementById('root'));window.commands=[];window.epoch=0;window.show=(data,legacy=false,reset=true)=>{if(reset)window.epoch++;const Component=legacy?Previous:ProjectRuns;root.render(<Component key={window.epoch} data={data} busy={false} send={async c=>{window.commands.push(c);return true;}}/>)};window.fields=data=>root.render(<DesktopWorkerFields data={data} projectId='project' app='workbuddy' worker={undefined} onChange={()=>{}}/>);`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:join(report,'bundle'),emptyOutDir:false,lib:{entry,name:'RunVersionUI',formats:['iife'],fileName:()=> 'ui.js'}}});
const worker=(app,version,bridge='bridge')=>({binding:{bridgeSha256:bridge,observation:{window:{app,version}}}});
function snapshot(desktops,states=[]) {
  return {desktopAccess:{bridgeSha256:'bridge',bindings:[]},codexJobs:[],projectRuns:[],openaiConnection:{keyConfigured:true},
    projectSetups:[{digest:'setup',setup:{projectId:'project',chiefModel:'chief',deputyModel:'deputy',managementMaxRequests:10}}],
    workflows:[{project:{id:'project',root:'D:\\fixture'},revisionDigest:'revision',approval:{},revision:{execution:desktops.map((desktopWorker,i)=>({taskId:'task'+i,desktopWorker}))},state:{tasks:Object.fromEntries(desktops.map((_,i)=>['task'+i,{state:states[i]??'pending'}]))}}]};
}
let browser,result;const cases=[],errors=[];let requests=0;
try {
  browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
  const page=await browser.newPage();page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push(e.message));
  await page.route('**/*',r=>{requests++;return r.abort();});
  await page.setContent('<html lang="zh-CN"><meta charset="utf-8"><div id="root"></div></html>');
  await page.addScriptTag({content:await readFile(join(report,'bundle/ui.js'),'utf8')});
  const start=()=>page.getByRole('button',{name:'启动已批准项目',exact:true});
  const render=async(data,legacy=false,reset=true)=>{
    const old=await page.evaluate(()=>window.epoch);
    await page.evaluate(({data,legacy,reset})=>window.show(data,legacy,reset),{data,legacy,reset});
    if(reset)await page.waitForFunction(old=>window.epoch>old,old);
    // Flush the React commit before checking either a new tree or existing modal.
    await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
  };
  const wb=snapshot([worker('workbuddy','5.7.7.0')]);
  await render(wb,true);assert.equal(await start().isDisabled(),true);cases.push('baseline reproduces supported WorkBuddy incorrectly blocked');
  await render(wb);assert.equal(await start().isEnabled(),true);cases.push('supported WorkBuddy can review start');
  await start().click();const confirm=()=>page.getByRole('button',{name:'确认启动调度',exact:true});
  await confirm().click();await page.waitForFunction(()=>window.commands.length===1);
  assert.equal((await page.evaluate(()=>window.commands[0])).type,'startProjectRun');cases.push('supported WorkBuddy emits one reviewed start command');
  await render(snapshot([worker('workbuddy','5.8.3.0')]));assert.equal(await start().isEnabled(),true);cases.push('adapted WorkBuddy 5.8.3 can review start');
  await render(snapshot([worker('qoder','0.4.3.0'),worker('workbuddy','5.8.3.0')]));assert.equal(await start().isEnabled(),true);cases.push('adapted WorkBuddy second pending task accepted');
  for(const [name,data,reason] of [
    ['new WorkBuddy version remains unsupported',snapshot([worker('workbuddy','5.8.2.0')]),/版本尚未接入/],
    ['other Qoder version remains unsupported',snapshot([worker('qoder','0.4.4.0')]),/版本尚未接入/],
    ['second pending desktop version is checked',snapshot([worker('qoder','0.4.3.0'),worker('workbuddy','5.8.2.0')]),/版本尚未接入/],
    ['second pending bridge identity is checked',snapshot([worker('qoder','0.4.3.0'),worker('workbuddy','5.7.7.0','other')]),/桥接版本变化/],
    ['unknown original task blocks start',snapshot([worker('workbuddy','5.7.7.0')],['unknown']),/结果待核对/],
    ['cancelling original task blocks start',snapshot([worker('workbuddy','5.7.7.0')],['cancelling']),/结果待核对/],
  ]) {await render(data);assert.equal(await start().isDisabled(),true,name);assert.match(await page.getByRole('status').innerText(),reason);cases.push(name);}
  const mixed=snapshot([worker('qoder','0.4.3.0'),worker('workbuddy','5.8.2.0')]);
  await render(mixed,true);assert.equal(await start().isEnabled(),true);cases.push('baseline reproduces skipped second desktop check');
  await render(snapshot([worker('qoder','0.4.3.0')]));assert.equal(await start().isEnabled(),true);cases.push('existing Qoder remains available');
  await render(snapshot([worker('workbuddy','5.7.7.0'),worker('qoder','0.4.3.0')]));assert.equal(await start().isEnabled(),true);cases.push('supported mixed pending desktop routes remain available');
  await render(snapshot([worker('workbuddy','5.8.2.0'),null],['accepted','pending']));assert.equal(await start().isEnabled(),true);cases.push('completed historical desktop does not block unrelated remaining task');
  await render(wb);await start().click();await confirm().waitFor();
  await render(snapshot([worker('workbuddy','5.8.2.0')]),false,false);assert.equal(await confirm().isDisabled(),true);cases.push('changed version invalidates open review');
  assert.equal(await page.evaluate(()=>window.commands.length),1);
  await page.evaluate(data=>window.fields(data),wb);await page.getByText(/副指挥分配的任务会等待你审阅并确认发送/).waitFor();cases.push('configuration text describes implemented dispatch');
  assert.deepEqual(errors,[]);assert.equal(requests,0);
  result={status:'passed',cases,syntheticCommands:1,networkRequests:requests,pageErrors:errors,actualModelCalls:0,actualDesktopSends:0};
}catch(e){result={status:'failed',cases,error:e.stack};process.exitCode=1;}
finally{if(browser)await browser.close();}
await writeFile(join(report,'result.json'),JSON.stringify({...result,browserClosed:!browser?.isConnected()},null,2)+'\n');
console.log(JSON.stringify({report,...result}));
