// Real WebView/native IPC with an offline Codex protocol fixture and isolated data.
import assert from 'node:assert/strict';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdir,mkdtemp,writeFile,readdir} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {chromium} from 'playwright-core';

assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const run=await mkdtemp(resolve('reports/connection-tests-native-'));
assert(process.env.CERULEAN_CACHE_SESSION,'Use managed cache runner');
const cache=join(process.env.CERULEAN_CACHE_SESSION,'connection-tests-native');
const data=join(run,'data');
await mkdir(cache,{recursive:true});
const fixture=join(cache,'codex.exe');
await writeFile(join(cache,'CodexFixture.cs'), `using System;using System.Collections.Generic;using System.Web.Script.Serialization;
class Fixture {
  static void Main() {
    var json=new JavaScriptSerializer(); string line;
    while((line=Console.ReadLine())!=null) {
      var request=json.Deserialize<Dictionary<string,object>>(line);
      if(!request.ContainsKey("id"))continue;
      object result=null;
      switch((string)request["method"]) {
        case "initialize":result=new {userAgent="offline-connection-fixture"};break;
        case "account/read":result=new {account=new {type="chatgpt"}};break;
        case "model/list":result=new {data=new[]{new {id="fixture",model="fixture",isDefault=true}},nextCursor=(object)null};break;
        case "thread/read":
          var parameters=(Dictionary<string,object>)request["params"];
          if((bool)parameters["includeTurns"])throw new Exception("Unexpected history read");
          result=new {thread=new {id="native-fixture-session"}};break;
        default:throw new Exception("Unexpected command");
      }
      Console.WriteLine(json.Serialize(new {id=request["id"],result=result}));Console.Out.Flush();
    }
  }
}`);
const compiler=join(process.env.SystemRoot,'Microsoft.NET/Framework64/v4.0.30319/csc.exe');
const compile=spawnSync(compiler,['/nologo','/r:System.Web.Extensions.dll',`/out:${fixture}`,join(cache,'CodexFixture.cs')],{windowsHide:true,encoding:'utf8'});
assert.equal(compile.status,0,compile.stdout+compile.stderr);
const wait=async predicate=>{for(let n=0;n<160;n++){const value=await predicate();if(value)return value;await new Promise(resolve=>setTimeout(resolve,150));}throw Error('Native condition timeout');};
const socket=createServer();socket.listen(0,'127.0.0.1');await once(socket,'listening');const port=socket.address().port;await new Promise(resolve=>socket.close(resolve));
const app=spawn(process.env.CERULEAN_NATIVE_GUI,[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(cache,'webviews'),CERULEAN_TEST_ACCESS_ROOT:join(run,'settings'),CERULEAN_TEST_MAINTENANCE_ROOT:join(run,'maintenance'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
let browser,page;
const cases=[];
const invoke=(name,args)=>page.evaluate(({name,args})=>window.__TAURI_INTERNALS__.invoke(name,args),{name,args});
try {
  browser=await wait(async()=>{assert.equal(app.exitCode,null);try{return await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});}catch{return null;}});
  page=await wait(()=>browser.contexts().flatMap(context=>context.pages()).find(page=>page.url().includes('tauri.localhost')));
  await page.getByText('本地核心已连接',{exact:true}).waitFor();
  const snapshot=await invoke('core_request',{command:{type:'snapshot'}});
  assert(snapshot.dataDirectory.replaceAll('\\','/').toLowerCase().endsWith(data.replaceAll('\\','/').toLowerCase()));
  const request={kind:'codexSession',sessionId:'native-fixture-session',executable:fixture};
  const result=await invoke('test_local_connection',{request});
  assert(result.message.includes('已读取所填会话标识'));
  assert(Number.isFinite(Date.parse(result.checkedAt)));
  cases.push('Real native command launches the offline protocol fixture, reads the exact session and returns a timed result');
  await assert.rejects(()=>invoke('test_local_connection',{request:{...request,sessionId:'wrong-session'}}),error=>String(error).includes('不一致'));
  await assert.rejects(()=>invoke('test_local_connection',{request:{...request,sessionId:'invalid/session'}}),error=>String(error).includes('identifier'));
  await assert.rejects(()=>invoke('test_local_connection',{request:{kind:'omp',executable:join(run,'missing.exe'),model:'deepseek-flash',runtimeParent:null}}));
  assert.equal((await readdir(data)).filter(name=>name.startsWith('connection-test-')).length,0);
  cases.push('Wrong session, invalid identifier and missing OMP executable fail, with temporary test directories cleaned');
  await invoke('core_submit',{command:{type:'checkCodex',executable:fixture}});
  await wait(async()=>{const next=await invoke('core_request',{command:{type:'snapshot'}});return next.codexConnection?.state==='available';});
  await page.locator('.sidebar nav').getByRole('button',{name:'接入管理',exact:false}).click();
  const chief=page.getByRole('region',{name:'主指挥',exact:true});
  await chief.locator('summary').filter({hasText:/^桌面端接入$/}).click();
  await chief.getByLabel('待测试的 Codex 会话标识').fill('native-fixture-session');
  await chief.getByRole('button',{name:'测试连通',exact:true}).click();
  await chief.getByRole('status').filter({hasText:'测试通过'}).waitFor();
  await chief.getByLabel('待测试的 Codex 会话标识').fill('wrong-session');
  assert.equal(await chief.getByRole('status').filter({hasText:'测试通过'}).count(),0);
  await chief.getByRole('button',{name:'测试连通',exact:true}).click();
  await chief.getByRole('alert').filter({hasText:'不一致'}).waitFor();
  cases.push('The actual desktop button displays native success and failure, and clears results when configuration changes');
  const worker=page.getByRole('region',{name:'执行端',exact:true});
  await worker.locator('summary').filter({hasText:/^API 连接$/}).click();
  await worker.getByLabel('OMP 程序完整路径').fill(join(run,'missing.exe'));
  await wait(async()=>Object.values(await invoke('access_settings_load')).includes(JSON.stringify(join(run,'missing.exe'))));
  await worker.getByRole('button',{name:'测试连通',exact:true}).click();
  const missingKey=page.getByRole('dialog',{name:'补充 OMP API Key',exact:true});
  await missingKey.waitFor();
  assert(await missingKey.getByRole('button',{name:'测试连通',exact:true}).isDisabled());
  await missingKey.getByRole('button',{name:'返回配置',exact:true}).click();
  cases.push('OMP input persists through the native allowlist; a missing Key opens the credential dialog before a connection attempt');
  await page.screenshot({path:join(run,'native-results.png'),fullPage:false});
  await page.getByRole('button',{name:'退出工作台',exact:true}).click();
  await page.getByRole('button',{name:'确认退出并停止后台',exact:true}).click();
  await wait(()=>app.exitCode!==null);
  await writeFile(join(run,'result.json'),JSON.stringify({status:'passed',cases,run,providerCalls:0,realCredentialsRead:false,guiExitCode:app.exitCode},null,2));
  console.log(JSON.stringify({status:'passed',cases:cases.length,run}));
} catch(error) {
  await page?.screenshot({path:join(run,'failure.png')}).catch(()=>{});
  await writeFile(join(run,'failure.json'),JSON.stringify({error:String(error),stack:error.stack,cases},null,2));
  throw error;
} finally {
  if(app.exitCode===null&&page&&!page.isClosed())await invoke('close_workbench').catch(()=>{});
  await browser?.close().catch(()=>{});
  await wait(()=>app.exitCode!==null);
}
