// The raw UIA provider is shared; only captured version/layout differences vary.
import {readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
export async function workbuddyFixture(report,version){
 assert.ok(['5.7.7.0','5.8.3.0'].includes(version));
 if(version==='5.7.7.0')return resolve('tests/fixtures/WorkBuddyStopWindow.cs');
 let source=await readFile('tests/fixtures/WorkBuddyStopWindow.cs','utf8');
 const paths='List=Panel+".1.0.0.0.0.0.0",Composer=Panel+".1.0.1.0.1.0"';assert.equal(source.split(paths).length,2);
 source=source.replaceAll('5.7.7.0',version).replace(paths,'List=Panel+".1.0.0.0.0.0.0.0",Composer=Panel+".1.0.0.1.0.1.0"');
 // Modern observations intentionally retain only the last three pairs. Mutate
 // a retained historical round when testing original-history mismatch.
 const history='Round(root,doc,1,"history-2","reply-2","已完成 2s")';assert.equal(source.split(history).length,2);
 source=source.replace(history,'Round(root,doc,1,mode=="history-changed"?"changed retained history":"history-2","reply-2","已完成 2s")');
 const path=join(report,'WorkBuddy583Provider.cs');await writeFile(path,source);return path;
}
