// Run in a fresh Node process with --permission and a single artifact read grant.
// The generated source has no imports, host callbacks, process, or filesystem in its VM context.
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
const source=readFileSync(process.argv[2],'utf8');
assert.ok(Buffer.byteLength(source)<=65536,'artifact exceeds 64 KiB');
const context=vm.createContext(Object.create(null),{codeGeneration:{strings:false,wasm:false}});
const module=new vm.SourceTextModule(source,{context});
await module.link(()=>{throw Error('imports are outside this task');});
await module.evaluate({timeout:1000});
assert.equal(typeof module.namespace.sum,'function');
// Calls also run inside the VM timeout, rather than calling generated functions in the host.
context.sum=module.namespace.sum;
const call=expression=>new vm.Script(expression).runInContext(context,{timeout:1000});
let checks=0;
for(const [expression,expected] of [['sum(2,3)',5],['sum(-7,4)',-3],['sum(0,0)',0],['sum(0.25,0.5)',0.75],['sum(10,-10)',0]]){
 assert.equal(call(expression),expected);checks++;
}
for(const expression of ['sum("2",3)','sum(2,"3")','sum(NaN,1)','sum(1,NaN)','sum(Infinity,1)','sum(1,-Infinity)','sum(null,1)','sum(1,undefined)','sum(true,1)','sum(1n,2)']){
 assert.equal(call(`(()=>{try{${expression};return false;}catch(e){return e instanceof TypeError;}})()`),true,expression);checks++;
}
console.log(JSON.stringify({status:'passed',checks}));
