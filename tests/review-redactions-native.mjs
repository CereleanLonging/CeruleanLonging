// Synthetic material only. Executes the packaged offline verifier, never a model.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {writeFileSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {combineReviewRedactions,defaultReviewRedactions} from '../apps/desktop/src/reviewRedactions.ts';
const [packagePath,reportPath]=process.argv.slice(2);
assert(packagePath&&reportPath,'Expected package directory and new report path');
const exe=join(resolve(packagePath),'cerulean-verifier.exe');
const invoke=operation=>{
  const result=spawnSync(exe,['--stdio-once'],{input:JSON.stringify({protocol:'cerulean-independent-verifier-v1',operation}),encoding:'utf8',windowsHide:true,timeout:15000,maxBuffer:1024*1024});
  assert.equal(result.status,0,result.stderr);assert(!result.error);
  return JSON.parse(result.stdout).result;
};
const snapshot={workflows:[{project:{id:'p',root:'D:/synthetic/project'}}],projectSetups:[{setup:{projectId:'p',chiefDirectory:'D:/synthetic/chief',workerDirectories:{worker:{relativePath:'executors/one'}}}}]};
const defaults=defaultReviewRedactions(snapshot,'p');
const rules=combineReviewRedactions(defaults,'private-value => PRIVATE_VALUE');
const cases=[];
const source=[...defaults.map(r=>r.literal),'private-value','return a + b;'].join('\n');
const prepare=(text=source,redactions=rules)=>({kind:'prepare',requirement:text,criteria:[text],files:[Array.from(Buffer.from(text))],redactions,objectiveChecks:1});
const first=invoke(prepare());assert(first.Ok,JSON.stringify(first));
const material=first.Ok.material;
for(const text of [material.requirement,...material.criteria,...material.fragments.map(f=>f.content)]){
  assert(!text.includes('synthetic'));assert(!text.includes('private-value'));assert(text.includes('PRIVATE_VALUE'));assert(text.includes('return a + b;'));
}
assert.equal(material.fragments[0].file,'file-0001');
assert(material.fragments[0].content.includes('REDACTED_DIR_03'));
cases.push('Real verifier applies generated parent/child, slash and escaped paths plus custom rules to requirements, criteria and source without changing unrelated code');
const second=invoke(prepare());assert.deepEqual(second,first);
cases.push('Identical approved rules and input produce deterministic material');
for(const [root,ordinary] of [[String.raw`\\?\D:\synthetic\project`,String.raw`D:\synthetic\project`],[String.raw`\\?\UNC\synthetic\share\project`,String.raw`\\synthetic\share\project`]]){
  const extended=defaultReviewRedactions({workflows:[{project:{id:'p',root}}]},'p');
  for(const literal of [root,ordinary,root.replaceAll('\\','\\\\'),ordinary.replaceAll('\\','\\\\')]){
    assert(extended.some(r=>r.literal===literal));
    const output=invoke(prepare(literal,combineReviewRedactions(extended,'')));
    assert(output.Ok);assert.equal(output.Ok.material.requirement,'REDACTED_DIR_01');
  }
}
cases.push('Windows extended drive/UNC roots also redact ordinary and escaped source paths');
const override=combineReviewRedactions(defaults,'D:/synthetic/project => MY_PROJECT');
assert(invoke(prepare('D:/synthetic/project/src/main.mjs',override)).Ok.material.requirement.includes('MY_PROJECT'));
cases.push('Custom exact-match replacement overrides the generated mapping');
for(const text of ['sk-synthetic-do-not-use','ghp_synthetic-do-not-use','vote_pass_next','text\u202econtrol','null\0byte'])assert(invoke(prepare(text,defaults)).Err);
cases.push('Default rules cannot disable existing secret, relay-marker or control-character rejection');
assert(invoke(prepare('sk-synthetic-do-not-use',combineReviewRedactions(defaults,'sk-synthetic-do-not-use => REMOVED_SECRET'))).Ok);
cases.push('A specific user rule can remove a synthetic detected secret before the mandatory final scan');
assert.throws(()=>combineReviewRedactions([],`原文${'界'.repeat(85)} => MASKED`));
assert.throws(()=>combineReviewRedactions([],'value => spaces not allowed'));
assert.throws(()=>combineReviewRedactions([],'abc => MASKED\nabc => OTHER'));
assert.throws(()=>combineReviewRedactions([],Array.from({length:65},(_,i)=>`literal-${i} => MASKED`).join('\n')));
assert.deepEqual(defaultReviewRedactions(snapshot,'unknown'),[]);
assert.deepEqual(defaultReviewRedactions({workflows:[{project:{id:'p',root:'D:/'}}]},'p'),[]);
cases.push('UTF-8 length, replacement, duplicate and count checks reject invalid rules; no whole-drive or invented project defaults');
const large=invoke(prepare(('const sum = a + b;\n').repeat(180),defaults));
assert(large.Ok.material.fragments.length>1);assert(large.Ok.material.fragments.every(f=>Buffer.byteLength(f.content)<=2048));
cases.push('Default/custom handling preserves the existing UTF-8 fragment size boundary');
writeFileSync(reportPath,JSON.stringify({passed:true,cases,exe,providerCalls:0,scope:'Current TypeScript rule generator plus real packaged offline verifier; synthetic inputs only'},null,2),{flag:'wx'});
console.log(JSON.stringify({passed:true,cases:cases.length,report:reportPath}));
