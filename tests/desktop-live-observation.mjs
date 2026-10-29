// Only enumerate and inspect known application windows; never activate or type.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/desktop-live-observation-'));
const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{windowsHide:true,encoding:'utf8',timeout:120000});assert.equal(build.status,0,build.stdout+'\n'+build.stderr);
const exe=build.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(exe);
const run=spawnSync(exe,['--exact','host::desktop_sessions::tests::live_desktop_observation','--ignored','--nocapture'],{windowsHide:true,encoding:'utf8',timeout:90000,env:{...process.env,CERULEAN_DESKTOP_OBSERVE_DIR:report}});
await writeFile(join(report,'test.log'),run.stdout+'\n'+run.stderr);assert.equal(run.status,0,run.stdout+'\n'+run.stderr);
const result=JSON.parse(await readFile(join(report,'result.json'),'utf8'));assert.equal(result.readOnly,true);
console.log(JSON.stringify({report,readOnly:true,windows:result.windows.map(w=>({app:w.window.app,version:w.window.version,observed:!!w.observation,error:w.error}))}));
