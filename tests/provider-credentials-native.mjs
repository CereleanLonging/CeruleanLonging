// Real Tauri/core/DPAPI, synthetic keys only. No authentication/model requests.
import {chromium} from 'playwright-core';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdir,mkdtemp,readFile,readdir,writeFile,stat} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import assert from 'node:assert/strict';
import {assertNativeRuntime} from './native-runtime.mjs';

await assertNativeRuntime();
await mkdir('reports',{recursive:true});
const dir=await mkdtemp(resolve('reports/provider-credentials-native-'));
const data=join(dir,'data');const runtime=resolve('target/debug/cerulean-core.exe');
const markers=['sk-offline-openai-native-only-marker','sk-offline-deepseek-native-only-marker','sk-offline-deepseek-native-replacement'];
let app,browser,page;const errors=[];const screenshots=[];
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function launch(){
  const server=createServer();server.listen(0,'127.0.0.1');await once(server,'listening');const port=server.address().port;await new Promise(r=>server.close(r));
  app=spawn(resolve('target/debug/cerulean-desktop.exe'),[],{windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(dir,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
  app.stdout.resume();app.on('error',e=>errors.push(e.message));app.stderr.on('data',b=>{if(b.toString().includes('panicked'))errors.push('native panic');});
  for(let i=0;i<80;i++){assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:1000});break;}catch{await pause(250);}}
  assert.ok(browser,'native browser connected');
  for(let i=0;i<50;i++){page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost'));if(page)break;await pause(200);}
  assert.ok(page);page.on('pageerror',e=>errors.push(e.message));await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});
  await page.getByRole('button',{name:'接入管理',exact:false}).click();
  await page.getByRole('region',{name:'DeepSeek 执行模型'}).waitFor();
}
async function close(){
  const exited=once(app,'exit');await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});
  await Promise.race([exited,new Promise((_,reject)=>setTimeout(()=>reject(Error('native close timed out')),8000).unref())]);
  await browser.close();app=null;page=null;browser=null;
}
function stopCore(){const r=spawnSync(runtime,['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:20000});assert.equal(r.status,0,r.stderr);}
const snap=()=>page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('core_request',{command:{type:'snapshot'}}));
const configured=(s,p)=>s.providerCredentials.find(x=>x.provider===p)?.keyConfigured;
function safe(s){const text=JSON.stringify(s);for(const marker of markers)assert.ok(!text.includes(marker));assert.equal(s.plans.length,0);assert.equal(s.workflows.length,0);assert.equal(s.managementDecisions.length,0);assert.equal(s.events.length,0);}
async function settled(){await page.waitForFunction(()=>!document.querySelector('#deepseek-key')?.closest('form')?.querySelector('button')?.disabled);}
async function saveDeepSeek(value){await page.locator('#deepseek-key').fill(value);await page.getByRole('button',{name:'加密保存 DeepSeek Key',exact:true}).click();await settled();assert.equal(await page.locator('#deepseek-key').inputValue(),'');}
async function shot(name){const path=join(dir,name);await page.getByRole('region',{name:'DeepSeek 执行模型'}).screenshot({path});screenshots.push(path);}
async function scan(directory){for(const entry of await readdir(directory,{withFileTypes:true})){const path=join(directory,entry.name);if(entry.isDirectory())await scan(path);else{if(entry.name.endsWith('.lock')){assert.equal((await stat(path)).size,0);continue;}const bytes=await readFile(path);for(const marker of markers)assert.ok(!bytes.includes(Buffer.from(marker)),`plaintext persisted in ${entry.name}`);}}}
try{
  await launch();let initial=await snap();safe(initial);assert.equal(configured(initial,'deepseek'),false);
  await page.locator('#openai-key').fill(markers[0]);await page.getByRole('button',{name:'加密保存 Key',exact:true}).click();await settled();
  assert.equal(await page.locator('#openai-key').inputValue(),'');
  await saveDeepSeek(markers[1]);let saved=await snap();safe(saved);assert.equal(configured(saved,'deepseek'),true);assert.equal(saved.openaiConnection.state,'unchecked');
  await shot('stored-unverified.png');
  const before=await readFile(join(data,'credentials/deepseek.dpapi'));
  await saveDeepSeek('invalid whitespace key');await page.getByRole('alert').filter({hasText:'不能带空格或换行'}).waitFor();
  assert.deepEqual(await readFile(join(data,'credentials/deepseek.dpapi')),before);safe(await snap());
  await saveDeepSeek(markers[2]);assert.notDeepEqual(await readFile(join(data,'credentials/deepseek.dpapi')),before);safe(await snap());
  await scan(data);
  await close();stopCore();await launch();
  let reopened=await snap();safe(reopened);assert.equal(configured(reopened,'deepseek'),true);assert.equal(configured(reopened,'openai'),true);
  assert.equal(await page.locator('#deepseek-key').inputValue(),'');await shot('reopened-unverified.png');
  await page.getByRole('button',{name:'删除本机 DeepSeek Key',exact:true}).click();await page.getByRole('button',{name:'删除本机 DeepSeek Key',exact:true}).waitFor({state:'visible'});await settled();
  let removed=await snap();safe(removed);assert.equal(configured(removed,'deepseek'),false);assert.equal(configured(removed,'openai'),true);
  await close();stopCore();await launch();
  let final=await snap();safe(final);assert.equal(configured(final,'deepseek'),false);assert.equal(configured(final,'openai'),true);await shot('deleted-reopened.png');await scan(data);
  assert.deepEqual(errors,[]);
  await writeFile(join(dir,'report.json'),JSON.stringify({status:'passed',realModelCalls:0,authenticationRequests:0,syntheticCredentialsOnly:true,fullCoreRestarts:2,checks:['stored-without-auth-claim','input-cleared','invalid-replacement-preserves-ciphertext','replacement','restart','independent-deletion','no-plaintext-in-product-data'],screenshots},null,2));
  console.log(JSON.stringify({status:'passed',dir,screenshots}));
}finally{
  if(app){if(page)await close().catch(()=>{});if(app?.exitCode===null)app.kill();}
  await browser?.close().catch(()=>{});
  const stopped=spawnSync(runtime,['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:20000});
  if(stopped.status!==0)console.error('Owned test core stop was not confirmed');
}
