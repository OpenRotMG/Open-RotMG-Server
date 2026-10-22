import test from 'node:test';
import assert from 'node:assert/strict';
import {once} from 'node:events';
import {startChild,stopChild,browserDiagnostic,runCase} from './native-test.mjs';
import {mkdtemp,writeFile,readFile,rename,rm} from 'node:fs/promises';
import {runInNewContext} from 'node:vm';
import {identity,registry} from './native-test-process.mjs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';

// Execute the production publication tail without launching a browser or duplicating its save logic.
async function publication(out,report,io={}) {
  const source=await readFile(new URL('./native-test.mjs',import.meta.url),'utf8');
  const save=source.match(/^  const save=async\(\)=>\{.*\};$/m)?.[0];
  const tail=source.match(/      participants\.push\(\{actor,account,connection,browser,chromePort,call,evaluate,bounds,display:env\.DISPLAY\}\);[\s\S]*?return participants\.at\(-1\);/)?.[0];
  assert(save&&tail,'Production addClient publication boundary must be present');
  const participants=[];
  return runInNewContext(`(()=>{${save};return {save,add:async client=>{
    const {actor,account,connection,browser,chromePort,call,evaluate,bounds,display}=client,env={DISPLAY:display};
    ${tail}
  }}})()`,{out,report,participants,join,elapsed:()=>123,options:{browserTools:'/synthetic/browser-tools'},writeFile,rename,driver:async command=>assert.deepEqual(JSON.parse(JSON.stringify(command)),{op:'pause'}),...io});
}
const participant=index=>({actor:index+1,account:'account-'+index,connection:{},browser:{owner:'native-test:synthetic',port:9000+index,target:'target-'+index,pid:100+index,browser:'browser-'+index,tabPolicy:'agent_only'},chromePort:9000+index,call(){},evaluate(){},bounds:{left:0,top:0,width:1280,height:960},display:':'+(index+100)});
const participantRecord=({actor,account,browser,bounds,display})=>({actor,account,browser,bounds,display});
const json=async path=>JSON.parse(await readFile(path,'utf8'));

test('addClient persists three in-scenario participants without another bounded save',async()=>{
  const out=await mkdtemp(join(tmpdir(),'native-test-three-')),report={status:'running',phase:'scenario',origin:'http://127.0.0.1:8000'},clients=[0,1,2].map(participant);
  try{
    const {add,save}=await publication(out,report);
    await add(clients[0]);await save();
    assert.equal((await json(join(out,'result.json'))).participants.length,1);
    await add(clients[1]);await add(clients[2]);
    assert.equal(report.participants.length,3);
    assert.equal((await json(join(out,'native.json'))).native.length,3);
    assert.deepEqual((await json(join(out,'result.json'))).participants,clients.map(participantRecord));
  }finally{await rm(out,{recursive:true});}
});

test('addClient publishes ordered participants before returning during a running scenario',async()=>{
  const out=await mkdtemp(join(tmpdir(),'native-test-publication-')),report={status:'running',phase:'scenario',origin:'http://127.0.0.1:8000'},clients=[0,1,2].map(participant);
  let beforeRename=async()=>{};
  try{
    const {add,save}=await publication(out,report,{rename:async(...args)=>{await beforeRename();return rename(...args);}});
    await add(clients[0]);await save();
    assert.equal((await json(join(out,'result.json'))).participants.length,1);
    for(let index=1;index<clients.length;index++){
      let release,entered;
      const blocked=new Promise(ok=>release=ok),atRename=new Promise(ok=>entered=ok);
      beforeRename=async()=>{entered();await blocked;};
      let returned=false;
      const pending=add(clients[index]).then(value=>{returned=true;return value;});
      try{
        await Promise.race([atRename,pending.then(()=>assert.fail('Join returned without publication'))]);
        assert.equal(returned,false);
        assert.equal((await json(join(out,'result.json'))).participants.length,index);
        assert.equal(report.participants.length,index+1);
        assert.equal((await json(join(out,'native.json'))).native.length,index+1);
      }finally{release();await pending;}
      const returnedClient=await pending;
      assert.equal(returnedClient.actor,clients[index].actor);
      for(const key of Object.keys(clients[index]))assert.equal(returnedClient[key],clients[index][key]);
      assert.deepEqual(await json(join(out,'result.json')),{...report,participants:clients.slice(0,index+1).map(participantRecord),wallMs:123});
      assert.deepEqual((await json(join(out,'native.json'))).native,clients.slice(0,index+1).map(c=>({port:c.chromePort,target:c.browser.target,actor:c.actor})));
    }
  }finally{await rm(out,{recursive:true});}
});

test('addClient rejects pause and publication errors without replacing the prior result',async t=>{
  for(const failure of ['pause','write','rename'])await t.test(failure,async()=>{
    const out=await mkdtemp(join(tmpdir(),'native-test-publication-error-')),report={status:'running',phase:'scenario',origin:'http://127.0.0.1:8000'};
    let fail=false;const error=Object.assign(Error('synthetic '+failure+' failure'),{code:failure==='pause'?'PAUSE':'ENOSPC'});
    try{
      const {add,save}=await publication(out,report,{
        driver:async()=>{if(fail&&failure==='pause')throw error;},
        writeFile:async(path,...args)=>{if(fail&&failure==='write'&&path===join(out,'result.tmp'))throw error;return writeFile(path,...args);},
        rename:async(...args)=>{if(fail&&failure==='rename')throw error;return rename(...args);}
      });
      await add(participant(0));await save();const previous=await readFile(join(out,'result.json'),'utf8');fail=true;
      await assert.rejects(add(participant(1)),value=>value===error);
      assert.equal(await readFile(join(out,'result.json'),'utf8'),previous);
      assert.equal((await json(join(out,'native.json'))).native.length,2);
    }finally{await rm(out,{recursive:true});}
  });
});

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
