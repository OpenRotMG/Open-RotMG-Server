import test from 'node:test';
import assert from 'node:assert/strict';
import {once} from 'node:events';
import {startChild,stopChild,browserDiagnostic,runCase,nativeConnections} from './native-test.mjs';
import {mkdtemp,writeFile,readFile,rename,rm,open,stat} from 'node:fs/promises';
import {setImmediate as nextTurn} from 'node:timers/promises';
import {runInNewContext} from 'node:vm';
import {identity,registry} from './native-test-process.mjs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';

// SOURCE-SYNTHETIC: production publication/cancellation seams, not runCase or native cleanup.
async function publication(out,report,io={}) {
  const source=await readFile(new URL('./native-test.mjs',import.meta.url),'utf8');
  const save=source.slice(source.indexOf('  const elapsed='),source.indexOf('  const register='));
  const cancellation=source.slice(source.indexOf('  const cleanupCauses='),source.indexOf('  const log='));
  const failure=source.match(/    primary=error;report\.status='failed';[\s\S]*?    await save\(\);/)?.[0];
  const finalSave=source.match(/    try\{await save\(\);\}finally\{process\.removeListener[^\n]+/)?.[0];
  const tail=source.match(/      participants\.push\(\{actor,account,connection,browser,chromePort,call,evaluate,bounds,display:env\.DISPLAY\}\);[\s\S]*?return participants\.at\(-1\);/)?.[0];
  assert(save.includes('const save=')&&cancellation.includes('const bounded=')&&failure&&finalSave&&tail,'Production publication boundaries must be present');
  const participants=[];
  return runInNewContext(`(()=>{
    const controller=new AbortController(),started=0,timeout=180000,previousOwner=undefined,archive=null,state=join(out,'state');
    let primary,phase='setup',closing=false,activeCase=true;
    ${save}${cancellation}
    return {save,bounded,interrupted,get primary(){return primary;},
      failure:async error=>{${failure}},finalSave:async()=>{${finalSave}},
      dispose:()=>clearTimeout(timer),add:async client=>{
        const {actor,account,connection,browser,chromePort,call,evaluate,bounds,display}=client,env={DISPLAY:display};
        ${tail}
      }};
  })()`,{out,report,participants,join,AbortController,Error,setTimeout:()=>null,clearTimeout(){},performance:{now:()=>123},process:{once(){},removeListener(){},env:{}},options:{browserTools:'/synthetic/browser-tools'},writeFile,rename,nativeConnections,driver:async command=>assert.deepEqual(JSON.parse(JSON.stringify(command)),{op:'pause'}),...io});
}
const participant=index=>({actor:index+1,account:'account-'+index,connection:{},browser:{owner:'native-test:synthetic',port:9000+index,target:'target-'+index,pid:100+index,browser:'browser-'+index,tabPolicy:'agent_only'},chromePort:9000+index,call(){},evaluate(){},bounds:{left:0,top:0,width:1280,height:960},display:':'+(index+100)});
const participantRecord=({actor,account,browser,bounds,display})=>({actor,account,browser,bounds,display});
const json=async path=>JSON.parse(await readFile(path,'utf8'));

test('addClient persists three in-scenario participants without another bounded save',async()=>{
  const out=await mkdtemp(join(tmpdir(),'native-test-three-')),report={status:'running',phase:'scenario',origin:'http://127.0.0.1:8000'},clients=[0,1,2].map(participant);
  try{
    const {add,save}=await publication(out,report);
    await add(clients[0]);await save();
    assert.equal((await json(join(out,'native.json'))).state,join(out,'state'));
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

test('bounded abort orders pending join, failure and final publications without losing caller errors',async t=>{
  for(const failedOperation of [null,'write','rename'])await t.test(failedOperation??'interrupted',async()=>{
    const out=await mkdtemp(join(tmpdir(),'native-test-abort-publication-'));
    const report={status:'running',phases:{},origin:'http://127.0.0.1:8000'};
    const ioError=Object.assign(Error('synthetic queued '+failedOperation),{code:'ENOSPC'}),failureError=Error('synthetic queued failure publication '+failedOperation);
    let release,entered,active=0,maxActive=0,renamedInodeWrites=0,held=false,joinReturned=false;
    const blocked=new Promise(ok=>release=ok),atWrite=new Promise(ok=>entered=ok),events=[];
    const outcome=promise=>promise.then(value=>({value}),error=>({error}));
    let seam,joining,failing;
    try{
      seam=await publication(out,report,{
        writeFile:async(path,bytes,options)=>{
          if(path!==join(out,'result.tmp'))return writeFile(path,bytes,options);
          active++;maxActive=Math.max(maxActive,active);
          const snapshot=JSON.parse(bytes),hold=snapshot.status==='running'&&snapshot.participants?.length===2;
          events.push(['write',snapshot]);assert.equal(options.mode,0o600);
          const file=await open(path,'w',options.mode);
          try{
            if(hold){held=true;entered();await blocked;
              const published=await stat(join(out,'result.json'));
              if((await file.stat()).ino===published.ino)renamedInodeWrites++;
              if(failedOperation==='write')throw ioError;
            }
            if(snapshot.status==='failed'&&!snapshot.cleanupErrors&&failedOperation==='write')throw failureError;
            await file.writeFile(bytes);
          }catch(error){active--;throw error;}finally{await file.close();}
        },
        rename:async(...args)=>{
          try{
            const snapshot=await json(args[0]);
            if(held&&snapshot.status==='running'&&failedOperation==='rename')throw ioError;
            if(snapshot.status==='failed'&&!snapshot.cleanupErrors&&failedOperation==='rename')throw failureError;
            await rename(...args);events.push(['rename',snapshot]);
          }finally{active--;}
        }
      });
      await seam.add(participant(0));
      const bounded=outcome(seam.bounded('scenario',()=>{
        joining=outcome(seam.add(participant(1)).then(client=>{joinReturned=true;return client;}));
        return joining.then(result=>{if(result.error)throw result.error;return result.value;});
      }));
      await Promise.race([atWrite,bounded.then(()=>assert.fail('Bounded work ended before join I/O'))]);seam.interrupted();
      const reason=(await bounded).error;
      assert.equal(String(reason),'Error: Native test interrupted');
      let failureSettled=false;
      failing=outcome(seam.failure(reason)).then(result=>{failureSettled=true;return result;});
      await nextTurn();const settledBeforeRelease=failureSettled;
      release();const joined=await joining,failed=await failing;
      report.cleanupErrors=[];await seam.finalSave();
      const final=await json(join(out,'result.json'));
      assert.equal(maxActive,1,'result.tmp publications must not overlap');
      assert.equal(settledBeforeRelease,false,'Failure publication must wait for join I/O');
      assert.equal(renamedInodeWrites,0,'Pending write must not modify a published inode');
      assert.equal(active,0);assert.equal(joinReturned,false);
      assert.equal(joined.error,failedOperation?ioError:reason);
      assert.equal(failed.error,failedOperation?failureError:undefined);
      assert.equal(seam.primary,reason);assert.equal(final.status,'failed');
      assert.deepEqual(final.failure,{phase:'scenario',error:String(reason),stack:reason.stack});
      assert.deepEqual(final.participants,[0,1].map(participant).map(participantRecord));
      assert.deepEqual(final.cleanupErrors,[]);
      assert.deepEqual(events.filter(([op])=>op==='write').map(([,s])=>[s.status,s.participants?.length,s.cleanupErrors]),[
        ['running',1,undefined],['running',1,undefined],['running',2,undefined],['failed',2,undefined],['failed',2,[]]
      ]);
      assert.equal((await stat(join(out,'result.json'))).mode&0o777,0o600);
    }finally{
      release();await Promise.all([joining,failing].filter(Boolean));seam?.dispose();await rm(out,{recursive:true});
    }
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

const {nativeJoins,spawnNativeFixture,runNativeScenario,nativeXauth}=await import('./native-test.mjs');
const deferred=()=>Promise.withResolvers();
function joinFixture(t){
  const abort=new AbortController(),events=[],participants=[],cleanup=[],diagnostics=[];let closing=false,next=0;
  const connections=nativeConnections({signal:abort.signal,closing:()=>closing,cleanupError:e=>cleanup.push(String(e)),diagnose:e=>diagnostics.push(e)});
  const operations={
    world:async()=>({result:{world:-11}}),port:async()=>12345,display:async()=>({DISPLAY:':private'}),
    connect:async()=>({closed:0,close(){this.closed++;}}),load:async()=>{},
    info:async()=>({spawn_points:[[4.5,5.5]]}),
  };
  const api=async command=>{events.push(command);if(command.op==='world')return operations.world();if(command.op==='world_info')return operations.info();assert.equal(command.op,'spawn');return{result:{actor:++next,account:next}};};
  const joins=nativeJoins({signal:abort.signal,api,participants,
    freePort:async()=>{events.push('port');return operations.port();},
    privateDisplay:async index=>{events.push(['display',index]);return operations.display();},
    openClient:async(spec,world)=>connections.open(async()=>{events.push('connect');return operations.connect();},async connection=>{
      events.push('use');const {created}=await spawnNativeFixture(api,spec,world,'synthetic-guest');await operations.load(connection);
      const client={actor:created.result.actor,connection};participants.push(client);return client;
    }),
  });
  const close=()=>{closing=true;connections.close();};t.after(close);
  return{abort,events,participants,cleanup,diagnostics,operations,joins,close};
}
test('native join accepts explicit templates and function positions without weakening fixture bounds',async t=>{
  const f=joinFixture(t),spec={...explicitFixture,world:'davy-jones-locker',position:world=>({x:world.spawn_points[0][0]+1,y:5.5})};
  await f.joins.add(spec);const spawn=f.events.find(e=>e.op==='spawn');
  assert.equal(spawn.spec.world,-11);assert.deepEqual(spawn.spec.position,{x:5.5,y:5.5});assert.equal(spawn.spec.controller,'native');assert.equal(spawn.spec.profile,null);
  assert.equal(f.participants.length,1);
});
test('native dungeon seeds preserve zero and stay out of character spawn requests',async t=>{
  for(const worldSeed of [0,Number.MAX_SAFE_INTEGER]){
    const f=joinFixture(t);await f.joins.add({...explicitFixture,world:'snake-pit',worldSeed});
    assert.deepEqual(f.events.find(e=>e.op==='world'),{op:'world',template:'snake-pit',seed:worldSeed});
    assert.equal(Object.hasOwn(f.events.find(e=>e.op==='spawn').spec,'worldSeed'),false);
  }
});
test('invalid native dungeon seeds reject before allocating worlds or browsers',async t=>{
  const f=joinFixture(t);
  for(const spec of [{...explicitFixture,worldSeed:0},...[null,-1,1.5,NaN,Infinity,Number.MAX_SAFE_INTEGER+1,'0'].map(worldSeed=>({...explicitFixture,world:'snake-pit',worldSeed}))]){
    await assert.rejects(f.joins.add(spec),/worldSeed requires/);assert.deepEqual(f.events,[]);
  }
});
test('every native join validates complete constant fixtures before world or resource effects',async t=>{
  const f=joinFixture(t);let hooks=0;
  const bad=[{}, {...explicitFixture,class:-1},{...explicitFixture,level:21},{...explicitFixture,seed:Number.MAX_SAFE_INTEGER+1},{...explicitFixture,equipment:[-1]},{...explicitFixture,equipment:[-2,0,0,0]},{...explicitFixture,backpack:[]},{...explicitFixture,stats:[1]},{...explicitFixture,stats:Array(8).fill(-1)},{...explicitFixture,world:{}},{...explicitFixture,position:{x:NaN,y:1}}];
  for(const change of [{class:65536},{class:1.5},{level:0},{level:1.5},{seed:-1},{equipment:[0,0,0,65536]},{backpack:[...Array(7).fill(-1),65536]},{stats:Array(8).fill(0.5)},{stats:Array(8).fill(2147483648)}])bad.push({...explicitFixture,...change});
  for(const spec of bad){Object.defineProperty(spec,'prepare',{value:()=>hooks++});await assert.rejects(f.joins.add(spec),/explicit fixture|fixture bounds/);assert.deepEqual(f.events,[]);}
  assert.equal(hooks,0);await f.joins.add(explicitFixture);assert.equal(f.participants.length,1);
});
test('resolved native world IDs and positions reject before spawn without substitution',async t=>{
  const f=joinFixture(t);f.operations.world=async()=>({result:{world:'bad'}});
  await assert.rejects(f.joins.add({...explicitFixture,world:'explicit-template'}),/explicit world ID/);
  assert.equal(f.events.length,1);assert.equal(f.events[0].op,'world');
  for(const position of [()=>null,()=>({x:Infinity,y:1}),'spawn']){
    f.operations.info=async()=>({spawn_points:[]});f.events.length=0;
    await assert.rejects(f.joins.add({...explicitFixture,position}),/finite position|spawn tuple/);
    assert.ok(!f.events.some(e=>e.op==='spawn'));
  }
});
test('failure between connect and LOAD closes all owned connections and retains primary and cleanup errors',async t=>{
  const f=joinFixture(t),connected=[];f.operations.connect=async()=>{const c={closed:0,close(){this.closed++;if(connected.indexOf(this)===1)throw Error('close failed');}};connected.push(c);return c;};
  await f.joins.add(explicitFixture);const primary=Error('LOAD failed');f.operations.load=async()=>{throw primary;};
  await assert.rejects(f.joins.add(explicitFixture),e=>e===primary);f.close();f.close();
  assert.deepEqual(connected.map(c=>c.closed),[1,1]);assert.deepEqual(f.cleanup,['Error: close failed']);assert.equal(f.participants.length,1);
});
test('escaped native join after abort has zero world port display or connection effects',async t=>{
  const f=joinFixture(t),escaped=f.joins.add;const reason=Error('closed case');f.abort.abort(reason);
  await assert.rejects(escaped(explicitFixture),e=>e===reason);assert.deepEqual(f.events,[]);
});
test('abort during native world or port await fences subsequent display and connection work',async t=>{
  for(const stage of ['world','port','display']){
    const f=joinFixture(t),entered=deferred(),held=deferred();f.operations[stage]=()=>{entered.resolve();return held.promise;};
    const pending=f.joins.add({...explicitFixture,world:stage==='world'?'explicit-template':-3});await entered.promise;
    const reason=Error('aborted '+stage);f.abort.abort(reason);held.resolve(stage==='world'?{result:{world:-11}}:12345);
    await assert.rejects(pending,e=>e===reason);assert.ok(!f.events.includes('connect'));if(stage!=='display')assert.ok(!f.events.some(Array.isArray));f.joins.idle();
  }
});
test('native xauth forwards the actual AbortSignal and terminates only its owned dummy exec',async()=>{
  const {execFile}=await import('node:child_process'),{promisify}=await import('node:util');const exec=promisify(execFile),abort=new AbortController(),ready=deferred();let child,closed;
  const pending=nativeXauth((command,args,options)=>{
    assert.equal(command,'xauth');assert.equal(options.signal,abort.signal);assert.deepEqual(args,['-f','synthetic-auth','add',':1','MIT-MAGIC-COOKIE-1','synthetic-cookie']);
    const work=exec(process.execPath,['-e','console.log("ready");setInterval(()=>{},1000)'],options);child=work.child;closed=new Promise(resolve=>child.once('close',resolve));child.stdout.once('data',()=>ready.resolve());return work;
  },'synthetic-auth',':1','synthetic-cookie',abort.signal);
  try{await ready.promise;abort.abort(Error('display cancelled'));await assert.rejects(pending,{name:'AbortError'});await closed;assert.equal(identity(child.pid),null);}finally{abort.abort();if(closed)await closed;}
  let called=false;await assert.rejects(nativeXauth(()=>{called=true;},'auth',':1','cookie',abort.signal),/display cancelled/);assert.equal(called,false);
});
test('late native connection after cleanup closes once before registration or CDP',async t=>{
  const f=joinFixture(t),entered=deferred(),held=deferred(),connection={closed:0,close(){this.closed++;}};
  f.operations.connect=()=>{entered.resolve();return held.promise;};const pending=f.joins.add(explicitFixture);await entered.promise;
  const reason=Error('case already closed');f.abort.abort(reason);f.close();held.resolve(connection);
  await assert.rejects(pending,e=>e===reason);f.close();assert.equal(connection.closed,1);assert.equal(f.participants.length,0);assert.ok(!f.events.includes('use'));
});
test('late native close failure preserves typed abort and close causes and visible diagnostics',async t=>{
  const f=joinFixture(t),entered=deferred(),held=deferred(),abort=Error('original abort'),failure=Error('late close failed');
  f.operations.connect=()=>{entered.resolve();return held.promise;};const pending=f.joins.add(explicitFixture);await entered.promise;
  f.abort.abort(abort);f.close();held.resolve({close(){throw failure;}});
  await assert.rejects(pending,error=>{assert(error instanceof AggregateError);assert.deepEqual(error.errors,[abort,failure]);return true;});
  assert.equal(f.diagnostics.length,1);assert.match(f.diagnostics[0],/original abort.*late close failed/);assert.deepEqual(f.cleanup,f.diagnostics);assert.ok(!f.events.includes('use'));
});
test('native joins reject concurrency before duplicate namespaces and clear guards for three sequential clients',async t=>{
  const f=joinFixture(t),entered=deferred(),held=deferred();f.operations.port=()=>{entered.resolve();return held.promise;};
  const first=f.joins.add(explicitFixture);await entered.promise;const before=f.events.length;
  await assert.rejects(f.joins.add(explicitFixture),/already pending/);assert.equal(f.events.length,before);
  held.resolve(12345);await first;f.operations.port=async()=>12346;await f.joins.add(explicitFixture);await f.joins.add(explicitFixture);
  assert.deepEqual(f.events.filter(Array.isArray),[['display',0],['display',1],['display',2]]);f.joins.idle();
  const count=f.events.length;await assert.rejects(f.joins.add(explicitFixture),/At most three/);assert.equal(f.events.length,count);f.joins.idle();
});
test('native case cannot complete while an unawaited join remains pending',async t=>{
  const f=joinFixture(t),held=deferred();f.operations.port=()=>held.promise;let pending;const report={status:'running'};
  await assert.rejects(runNativeScenario(()=>{pending=f.joins.add(explicitFixture);pending.catch(()=>{});},{observe:async()=>({game_ms:0})},()=>f.joins.idle(),report),/pending join/);
  assert.equal(report.status,'running');assert.equal(report.after,undefined);f.abort.abort(Error('case rejected'));held.resolve(12345);await assert.rejects(pending,/case rejected/);
});
test('native case rejects a join started during final observation before any passed result',async t=>{
  const f=joinFixture(t),held=deferred();f.operations.port=()=>held.promise;let observations=0,pending;const report={status:'running'};
  await assert.rejects(runNativeScenario(async()=>{},{observe:async()=>{if(++observations===2){pending=f.joins.add(explicitFixture);pending.catch(()=>{});}return{game_ms:observations};}},()=>f.joins.idle(),report),/pending join/);
  assert.equal(report.status,'running');f.abort.abort(Error('case rejected'));held.resolve(12345);await assert.rejects(pending,/case rejected/);
});
test('native scenario preserves the required run callback module receiver and game identity',async()=>{
  const report={},game={observe:async()=>({game_ms:1})};let ran=false,idle=0;
  const module={async run(actual){assert.equal(this,module);assert.equal(actual,game);ran=true;}};
  await runNativeScenario(game=>module.run(game),game,()=>idle++,report);assert(ran);assert.equal(idle,2);assert.deepEqual(report.before,report.after);
});
test('already cancelled native connection scope invokes neither connect nor use',async()=>{
  const reason=Error('scope cancelled before connect');let calls=0;
  const owned=nativeConnections({signal:AbortSignal.abort(reason),closing:()=>true,cleanupError:()=>assert.fail('unexpected cleanup'),diagnose:()=>assert.fail('unexpected diagnostic')});
  await assert.rejects(owned.open(()=>{calls++;},()=>{calls++;}),error=>error===reason);assert.equal(calls,0);owned.close();
});
