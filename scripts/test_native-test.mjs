import test from 'node:test';
import assert from 'node:assert/strict';
import {once} from 'node:events';
import {startChild,stopChild,browserDiagnostic,runCase} from './native-test.mjs';
import {mkdtemp,writeFile,readFile,rm} from 'node:fs/promises';
import {identity,registry} from './native-test-process.mjs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';

test('existing run directories are never reset',async()=>{
  const dir=await mkdtemp(join(tmpdir(),'native-test-existing-'));
  try{await writeFile(join(dir,'retained'),'original');await assert.rejects(runCase('/unused',{runDir:dir,browserTools:'/unused',chrome:'/unused',chromeArgs:[],timeoutMs:180000}),{code:'EEXIST'});assert.equal(await readFile(join(dir,'retained'),'utf8'),'original');}finally{await rm(dir,{recursive:true});}
});
test('an already-cancelled run cannot launch a child',()=>{
  assert.throws(()=>startChild(process.execPath,['-e','process.exit(0)'],{},AbortSignal.abort(Error('stopped'))),/stopped/);
});
test('cleanup escalates and reaps a child that ignores graceful signals',async()=>{
  const row=startChild(process.execPath,['-e','process.on("SIGINT",()=>{});process.on("SIGTERM",()=>{});console.log("ready");setInterval(()=>{},1000)'],{stdio:['ignore','pipe','pipe']},new AbortController().signal);
  try{await once(row.child.stdout,'data');const result=await stopChild(row,20);assert.equal(result.signal,'SIGKILL');assert.throws(()=>process.kill(row.child.pid,0),{code:'ESRCH'});}finally{if(!row.done)await stopChild(row,20);}
});
test('the independent guardian cleans owned processes after their runner disappears',async()=>{
  const dir=await mkdtemp(join(tmpdir(),'native-test-guardian-')),signal=new AbortController().signal;
  const parent=startChild(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'},signal);
  const child=startChild(process.execPath,['-e','process.on("SIGINT",()=>{});process.on("SIGTERM",()=>{});setInterval(()=>{},1000)'],{stdio:'ignore'},signal);
  let guard;
  try{
    await Promise.all([parent.started,child.started]);
    await writeFile(join(dir,'result.json'),JSON.stringify({status:'running'}));
    registry(join(dir,'children.json'),{parent:identity(parent.child.pid),boot:await readFile('/proc/sys/kernel/random/boot_id','utf8'),deadline:Date.now()+30000,children:[identity(child.child.pid)],done:false});
    guard=startChild(process.execPath,[new URL('./native-test-process.mjs',import.meta.url).pathname,dir],{stdio:'ignore'},signal);
    parent.child.kill('SIGKILL');await parent.closed;const result=await guard.closed;
    assert.equal(result.code,0);assert.equal(identity(child.child.pid),null);assert.deepEqual(JSON.parse(await readFile(join(dir,'guardian.json'),'utf8')).remaining,[]);
    assert.equal(JSON.parse(await readFile(join(dir,'result.json'),'utf8')).failure.phase,'guardian');
  }finally{for(const row of [guard,child,parent].filter(Boolean))await stopChild(row,20);await rm(dir,{recursive:true});}
});
test('a failed spawn reports its actual launch error',async()=>{
  const row=startChild('/nonexistent/native-test-command',[],{},new AbortController().signal);
  await assert.rejects(row.started,{code:'ENOENT'});await row.closed;
});
test('browser-only diagnostics stay separate while active renderer errors fail',()=>{
  const p='[1:2:1020/120000.000001:ERROR:';
  assert.equal(browserDiagnostic(p+'google_apis/gcm/engine/registration_request.cc:291] Registration response error message: PHONE_REGISTRATION_ERROR'),'background-push');
  assert.equal(browserDiagnostic(p+'google_apis/gcm/engine/mcs_client.cc:702]   Error code: 401  Error message: Authentication Failed: wrong_secret'),'background-push');
  assert.equal(browserDiagnostic(p+'wrong.cc:291] Registration response error message: DEPRECATED_ENDPOINT'),'fatal');
  const gpu=p+'gpu/command_buffer/service/shared_image/shared_image_manager.cc:260] SharedImageManager::ProduceSkia: Trying to Produce a Skia representation from a non-existent mailbox.';
  assert.equal(browserDiagnostic(gpu),'fatal');assert.equal(browserDiagnostic(gpu,true),'renderer-shutdown');
  assert.equal(browserDiagnostic('thread worker panicked at failure',true),'fatal');
});

const explicitFixture={class:0x30e,level:19,seed:12351,world:-3,position:{x:4.5,y:5.5},equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:null};
async function prepared(module,api=async()=>({paused:true}),signal=new AbortController().signal){
  const native=await import('./native-test.mjs');
  return native.prepareFixture(module,api,signal);
}
test('prepared fixture preserves the existing constant case without API calls',async()=>{
  assert.equal(await prepared({fixture:explicitFixture},()=>assert.fail('unexpected API call')),explicitFixture);
});
test('prepared fixture hook awaits observed preparation before native fixture use',async()=>{
  const order=['server-ready'];const observed={...explicitFixture,position:{x:8.5,y:9.5}};
  const fixture=await prepared({fixture:explicitFixture,async prepare(api){order.push('prepare');await api({op:'inspect',actor:1});order.push('return');return observed;}},async command=>{assert.deepEqual(command,{op:'inspect',actor:1});order.push('inspect');});
  order.push('browser');assert.equal(fixture,observed);assert.deepEqual(order,['server-ready','prepare','inspect','return','browser']);
});
test('prepared fixture rejects invalid returns and never uses the constant fixture instead',async()=>{
  for(const value of [undefined,null,{}, {...explicitFixture,level:21},{...explicitFixture,position:{x:NaN,y:1}},{...explicitFixture,equipment:[]},{...explicitFixture,stats:[1]}])await assert.rejects(prepared({fixture:explicitFixture,prepare:async()=>value}),/explicit fixture|fixture bounds/);
});
test('prepared fixture hook errors propagate unchanged',async()=>{
  const error=Error('observed quest missing');await assert.rejects(prepared({fixture:explicitFixture,prepare:async()=>{throw error;}}),e=>e===error);
  await assert.rejects(prepared({fixture:explicitFixture,prepare:1}),/prepare must be a function/);
});
test('prepared fixture permits only paused read operations and explicit direct scout spawn',async()=>{
  let calls=0;const commands=[{op:'inspect',actor:1},{op:'world_info',world:-3},{op:'spawn',spec:{controller:'direct',profile:null}}];
  await prepared({async prepare(api){for(const command of commands)await api(command);return explicitFixture;}},async()=>calls++);assert.equal(calls,3);
  for(const command of [{op:'act'},{op:'resume'},{op:'cheat'},{op:'world'},{op:'spawn',spec:{controller:'game_bot',profile:'Pixl'}}])await assert.rejects(prepared({async prepare(api){await api(command);return explicitFixture;}},()=>assert.fail('forbidden API executed')),/preparation allows only/);
});
test('prepared fixture cancellation bounds a pending hook and fences subsequent API use',async()=>{
  const abort=new AbortController();let api;const pending=prepared({prepare:command=>{api=command;return new Promise(()=>{});}},()=>assert.fail('cancelled API executed'),abort.signal);
  await new Promise(ok=>setImmediate(ok));abort.abort(Error('preparation deadline'));await assert.rejects(pending,/preparation deadline/);await assert.rejects(api({op:'inspect',actor:1}),/preparation deadline/);
});
test('prepared fixture seals escaped API access after success and error',async()=>{
  for(const failure of [false,true]){
    let access,calls=0;const module={prepare:api=>{access=api;if(failure)throw Error('preparation failed');return explicitFixture;}};
    if(failure)await assert.rejects(prepared(module,()=>calls++),/preparation failed/);else await prepared(module,()=>calls++);
    await assert.rejects(access({op:'inspect',actor:1}),/preparation phase is closed/);assert.equal(calls,0);
  }
});
test('prepared fixture seals an in-flight API reply after hook settlement',async()=>{
  let release,pending;await prepared({prepare:api=>{pending=api({op:'inspect',actor:1});return explicitFixture;}},()=>new Promise(ok=>release=ok));
  release({paused:true});await assert.rejects(pending,/preparation phase is closed/);
});
