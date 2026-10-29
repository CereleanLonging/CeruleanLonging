// Actual Tauri/IPC/DPAPI and asynchronous local credential failure; no external API.
import {chromium} from 'playwright-core';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdtemp,readFile,readdir,writeFile,stat} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
import {assertNativeRuntime} from './native-runtime.mjs';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');await assertNativeRuntime();
const report=await mkdtemp(resolve('reports/provider-discovery-native-')),data=join(report,'data');
const core=resolve('target/debug/cerulean-core.exe');
const providers=[['deepseek','DeepSeek','DeepSeek 执行模型'],['anthropic','Claude','Claude API'],['qwen','Qwen','Qwen / 百炼 API'],['kimi','Kimi','Kimi API（中国站）'],['grok','Grok','xAI Grok API']];
const markers=providers.map(([p])=>`sk-offline-native-${p}-not-real`);const errors=[];let app,browser,page;
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,label){for(let n=0;n<180;n++){const v=await fn();if(v)return v;await sleep(100);}throw Error('Timed out: '+label);}
const request=command=>page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command);
const snapshot=()=>request({type:'snapshot'});
async function launch(){
 const server=createServer().listen(0,'127.0.0.1');await once(server,'listening');const port=server.address().port;await new Promise(r=>server.close(r));
 app=spawn(resolve('target/debug/cerulean-desktop.exe'),[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(report,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
 await until(async()=>{assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});return true;}catch{return false;}},'WebView');
 page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')),'page');page.on('pageerror',e=>errors.push(e.message));
 await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});await page.getByRole('button',{name:'接入管理',exact:false}).click();
}
async function close(){const exited=once(app,'exit');await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});await Promise.race([exited,new Promise((_,reject)=>setTimeout(()=>reject(Error('window exit timeout')),8000).unref())]);await browser.close();browser=null;app=null;page=null;const r=spawnSync(core,['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:20000});assert.equal(r.status,0,r.stderr);}
async function settle(p){await until(async()=>!(await page.getByRole('button',{name:`加密保存 ${providers.find(x=>x[0]===p)[1]} Key`,exact:true}).isDisabled()),'form settled');}
async function save([p,name],value){await page.locator(`#${p}-key`).fill(value);await page.getByRole('button',{name:`加密保存 ${name} Key`,exact:true}).click();await settle(p);assert.equal(await page.locator(`#${p}-key`).inputValue(),'');}
function safe(s){for(const marker of markers)assert.ok(!JSON.stringify(s).includes(marker));assert.equal(s.events.length,0);assert.equal(s.workflows.length,0);assert.equal(s.managementDecisions.length,0);assert.equal(s.plans.length,0);}
async function scan(dir){for(const f of await readdir(dir,{withFileTypes:true})){const p=join(dir,f.name);if(f.isDirectory())await scan(p);else if(f.name.endsWith('.lock'))assert.equal((await stat(p)).size,0);else{const bytes=await readFile(p);for(const marker of markers)assert.ok(!bytes.includes(Buffer.from(marker)),f.name);}}}
try{
 await launch();assert.equal((await snapshot()).providerConnections.length,5);
 for(let i=0;i<providers.length;i++){
  const entry=providers[i],[p,,title]=entry;const region=page.getByRole('region',{name:title,exact:true});await region.waitFor();
  await save(entry,markers[i]);assert.equal((await snapshot()).providerConnections.find(s=>s.provider===p).state,'unchecked');
  const encrypted=await readFile(join(data,`credentials/${p}.dpapi`));
  await save(entry,'invalid whitespace key');assert.deepEqual(await readFile(join(data,`credentials/${p}.dpapi`)),encrypted);
 }
 const qwen=page.getByRole('region',{name:'Qwen / 百炼 API',exact:true});const check=qwen.getByRole('button',{name:'读取 Qwen 模型列表',exact:true});assert.equal(await check.isDisabled(),true);
 await page.locator('#qwen-workspace').fill('workspace-fixture');assert.equal(await check.isDisabled(),false);await qwen.screenshot({path:join(report,'qwen-region.png')});
 const bad=await page.evaluate(async()=>{try{await window.__TAURI_INTERNALS__.invoke('core_request',{command:{type:'checkProviderModels',target:{kind:'qwenBeijing',workspaceId:'x@evil.test'}}});return false;}catch{return true;}});assert.equal(bad,true);
 await page.locator('#qwen-region').selectOption('qwenSingapore');assert.equal(await check.isDisabled(),false);assert.equal((await snapshot()).providerConnections.find(s=>s.provider==='qwen').state,'unchecked');
 await scan(data);safe(await snapshot());await close();await launch();
 let reopened=await snapshot();assert.ok(reopened.providerConnections.every(s=>s.keyConfigured&&s.state==='unchecked'&&s.checkedAt===null));safe(reopened);
 // Corrupt only this isolated synthetic credential. The real probe thread stops
 // at DPAPI decryption, before any HTTP client or request is constructed.
 await writeFile(join(data,'credentials/grok.dpapi'),'invalid encrypted test fixture');
 await page.getByRole('button',{name:'读取 Grok 模型列表',exact:true}).click();
 const grok=page.getByRole('region',{name:'xAI Grok API',exact:true});await grok.getByText('检查未通过',{exact:true}).waitFor();await grok.getByRole('alert').filter({hasText:'Windows 凭证加解密失败'}).waitFor();
 await grok.screenshot({path:join(report,'grok-local-failure.png')});let checked=await snapshot();assert.equal(checked.providerConnections.find(s=>s.provider==='grok').endpoint,'https://api.x.ai/v1/models');safe(checked);
 await save(providers[4],markers[4]);assert.equal((await snapshot()).providerConnections.find(s=>s.provider==='grok').checkedAt,null);
 await page.getByRole('button',{name:'删除本机 Kimi Key',exact:true}).click();await settle('kimi');
 await close();await launch();const final=await snapshot();safe(final);assert.ok(final.providerConnections.every(s=>s.keyConfigured===(s.provider!=='kimi')&&s.state===(s.provider==='kimi'?'notConfigured':'unchecked')));
 await scan(data);assert.deepEqual(errors,[]);await close();
 await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',at:new Date().toISOString(),providers:providers.map(p=>p[0]),realModelCalls:0,externalApiRequests:0,syntheticCredentialsOnly:true,fullCoreRestarts:2,invalidReplacementPreserved:true,independentDeletion:true,plaintextAbsent:true,qwenRegionExplicit:true,invalidWorkspaceRejected:true,asyncLocalCredentialFailure:true,replacementClearsCheck:true},null,2));console.log(JSON.stringify({report,status:'passed'}));
}catch(error){await page?.screenshot({path:join(report,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(report,'failure.json'),JSON.stringify({error:String(error),errors}));throw error;}
finally{await browser?.close().catch(()=>{});if(app&&app.exitCode===null){const exited=once(app,'exit');app.kill();await exited;}}
