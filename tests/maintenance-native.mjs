import {chromium} from 'playwright-core';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdtemp,readFile,writeFile,access} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const run=await mkdtemp(resolve('reports/maintenance-native-'));
const data=join(run,'data'),cache=join(run,'maintenance-cache');
const server=createServer();server.listen(0,'127.0.0.1');await once(server,'listening');const port=server.address().port;await new Promise(r=>server.close(r));
const sha=b=>createHash('sha256').update(b).digest('hex');
const runtime=resolve('target/debug/cerulean-runtime.exe');
assert.equal(sha(await readFile(runtime)),sha(await readFile('target/debug/cerulean-core.exe')));
const app=spawn(resolve('target/debug/cerulean-desktop.exe'),[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(run,'webviews'),CERULEAN_TEST_MAINTENANCE_ROOT:cache,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
const sleep=ms=>new Promise(r=>setTimeout(r,ms));let browser,session,closed=false;
function coreStatus(){const r=spawnSync(runtime,['--workbench','--data-dir',data,'status'],{windowsHide:true,encoding:'utf8',timeout:15000});assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);}
function windowStatus(close=false){
  const script=`$ErrorActionPreference='Stop'
$targetId=[int]$env:CERULEAN_PROBE_WINDOW_PID
$record=Get-CimInstance Win32_Process -Filter ("ProcessId = "+$targetId)
if(-not $record){@{state='exited'}|ConvertTo-Json -Compress;exit 0}
if(-not $record.CommandLine.Contains($env:CERULEAN_PROBE_SESSION_DIR) -or -not $record.CommandLine.Contains('maintenance-window.ps1')){throw 'Process ownership differs'}
$p=Get-Process -Id $targetId
if($p.MainWindowHandle -eq 0){@{state='waiting'}|ConvertTo-Json -Compress;exit 0}
if(-not $p.MainWindowTitle.StartsWith('CeruleanLonging')){throw 'Unexpected own window title'}
if($env:CERULEAN_PROBE_CLOSE -eq '1'){if(-not $p.CloseMainWindow()){throw 'Own window refused close'};if(-not $p.WaitForExit(10000)){throw 'Own maintenance process still active; do not kill it'};@{state='closed'}|ConvertTo-Json -Compress}else{@{state='ready';pid=$p.Id;title=$p.MainWindowTitle}|ConvertTo-Json -Compress}`;
  const r=spawnSync('powershell.exe',['-NoProfile','-EncodedCommand',Buffer.from(script,'utf16le').toString('base64')],{windowsHide:true,encoding:'utf8',timeout:20000,env:{...process.env,CERULEAN_PROBE_WINDOW_PID:String(session.pid),CERULEAN_PROBE_SESSION_DIR:session.directory,CERULEAN_PROBE_CLOSE:close?'1':'0'}});
  assert.equal(r.status,0,r.stderr);return JSON.parse(r.stdout);
}
try{
  for(let i=0;i<80;i++){assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:800});break;}catch{await sleep(200);}}
  assert.ok(browser);let page;
  for(let i=0;i<80;i++){page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost'));if(page)break;await sleep(150);}
  assert.ok(page);await page.getByRole('button',{name:'维护与升级',exact:false}).click();
  const open=page.getByRole('button',{name:'打开独立维护窗口'});await open.waitFor();const originalCore=coreStatus();await open.click();
  await page.getByRole('status').waitFor({timeout:30000});await page.getByText('本次维护副本与恢复信息').click();
  const status=await page.getByRole('status').innerText();const details=page.getByRole('status').locator('details p');
  session={pid:Number(status.match(/进程 (\d+)/)[1]),directory:await details.nth(0).innerText(),sessionDigest:(await details.nth(1).innerText()).match(/[a-f0-9]{64}/)[0]};
  assert.ok(session.directory.startsWith(cache));
  const bytes=await readFile(join(session.directory,'session.json'));assert.equal(sha(bytes),session.sessionDigest);
  const config=JSON.parse(bytes);assert.equal(config.version,'0.1.3');assert.equal(config.dataDirectory,data);
  for(const entry of config.artifacts){assert.equal(sha(await readFile(join(session.directory,entry.path))),entry.sha256);}
  assert.equal(sha(await readFile(join(session.directory,'cerulean-runtime.exe'))),sha(await readFile(runtime)));
  let native;
  for(let i=0;i<20;i++){native=windowStatus();if(native.state==='ready')break;if(native.state==='exited')throw Error('Maintenance window exited before readiness');await sleep(200);}
  assert.equal(native.state,'ready');
  await assert.rejects(access(join(session.directory,'operations')));await assert.rejects(access(join(data,'upgrade-hold.json')));
  assert.equal(coreStatus().serverPid,originalCore.serverPid);
  await page.screenshot({path:join(run,'maintenance-entry-native.png')});
  assert.equal(windowStatus(true).state,'closed');closed=true;
  const result={status:'passed',run,version:config.version,session,native,embeddedToolsVerified:3,runtimeCopyVerified:true,noMaintenanceActionsOnOpen:true,coreNotStopped:true,maintenanceWindowClosed:true,realModelCalls:0,userDesktopActions:0,ownedProductWindows:2};
  await writeFile(join(run,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
}finally{
  if(session&&!closed){try{windowStatus(true);}catch(e){console.error('Own maintenance window cleanup not confirmed:',e.message);}}
  if(browser)await browser.close().catch(()=>{});
  if(app.exitCode===null){app.kill();await once(app,'exit');}
  const stop=spawnSync(runtime,['--workbench','--data-dir',data,'service','stop'],{windowsHide:true,encoding:'utf8',timeout:15000});
  if(stop.status!==0)console.error('Own core cleanup not confirmed:',stop.stderr);
}
