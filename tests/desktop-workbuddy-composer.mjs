// Actual UIA against owned WPF windows; no synthetic observer responses.
import {spawn,spawnSync} from 'node:child_process';
import {mkdtemp,mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/desktop-workbuddy-composer-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
function build(name,sources,refs){
 const exe=join(report,name+'.exe');
 const result=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/r:System.Web.Extensions.dll','/r:System.Xaml.dll',...refs.map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,...sources.map(p=>resolve(p))],{encoding:'utf8',windowsHide:true,timeout:30000});
 assert.equal(result.status,0,result.stdout+result.stderr);return exe;
}
const bridge=build('Observer',['crates/core/native/DesktopObserver.cs','crates/core/native/DesktopTrial.cs','crates/core/native/DesktopStop.cs'],['UIAutomationClient','UIAutomationTypes','WindowsBase']);
const fixture=build('WorkBuddy',['tests/fixtures/WorkBuddyComposerWindow.cs'],['UIAutomationTypes','UIAutomationProvider','WindowsBase','PresentationFramework','PresentationCore']);
const cases=[];
for(const [mode,expected] of [['placeholder',null],['empty',null],['format-empty',null],['draft','draft_or_composer_unavailable'],['wrapped-draft','draft_or_composer_unavailable'],['space','draft_or_composer_unavailable'],['auxiliary-draft','draft_or_composer_unavailable'],['large-auxiliary','draft_or_composer_unavailable'],['missing','draft_or_composer_unavailable'],['duplicate','draft_or_composer_unavailable'],['busy','session_busy_or_approval_pending'],...['selected','duplicate','hidden','disabled','text','empty','control','long'].map(m=>['model-'+m,null])]) {
 const dir=join(report,mode);await mkdir(dir);
 const child=spawn(fixture,[dir,mode],{windowsHide:true,stdio:'ignore'});
 const exited=new Promise((resolve,reject)=>{child.once('exit',resolve);child.once('error',reject);});
 try {
  let identity;const deadline=Date.now()+10000;
  while(!identity){try{identity=JSON.parse(await readFile(join(dir,'window.json'),'utf8'));}catch(e){assert.ok(Date.now()<deadline,'fixture window timeout');await new Promise(r=>setTimeout(r,50));}}
  assert.equal(identity.pid,child.pid);
  const run=spawnSync(bridge,[JSON.stringify({version:1,op:'inspect',window:identity})],{windowsHide:true,encoding:'utf8',timeout:15000});
  assert.equal(run.error,undefined,run.error?.message);
  const reply=JSON.parse(run.stdout.replace(/^\uFEFF/,''));
  assert.equal(reply.ok,expected===null,mode+': '+run.stdout);
  if(expected){assert.equal(reply.error,expected,mode);assert.equal(run.status,1);}else{assert.equal(reply.result.observation.draftEmpty,true);assert.equal(reply.result.observation.sessionId,null);assert.equal(reply.result.observation.modelLabel,mode==='model-selected'?'Hy3':null,mode);assert.equal(run.status,0);}
  if(mode==='model-selected') {
   const prepare=spawnSync(bridge,[JSON.stringify({version:1,op:'taskprepare',window:identity})],{windowsHide:true,encoding:'utf8',timeout:15000});
   assert.equal(prepare.status,1);assert.equal(JSON.parse(prepare.stdout.replace(/^\uFEFF/,'')).error,'trial_history_unavailable','model selection alone must not authorize history preparation');
  }
  assert.equal(await readFile(join(dir,'changes.json'),'utf8'),'0');
  cases.push({mode,expected,error:reply.error??null,modelLabel:reply.result?.observation?.modelLabel??null,passed:true,inputChanges:0});
  await writeFile(join(dir,'observation.json'),JSON.stringify(reply,null,2)+'\n');
 } finally {child.kill();await exited;}
}
const result={status:'passed',realUia:true,ownedWindows:true,externalMessages:0,cases};
await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({report,...result}));
