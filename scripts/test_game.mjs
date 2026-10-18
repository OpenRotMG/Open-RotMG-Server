import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, mkdir, writeFile, readFile, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {createServer} from 'node:http';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';

const exec=promisify(execFile), script=fileURLToPath(new URL('./game.mjs',import.meta.url));
async function fixture(t,transfer=false) {
  const dir=await mkdtemp(join(tmpdir(),'openrotmg-game-test-')), calls=[];
  await mkdir(join(dir,'testing'));
  await writeFile(join(dir,'control.token'),'fixture-control');
  await writeFile(join(dir,'testing.json'),JSON.stringify({schema:1,provenance:'prepared'}));
  let paused=true,world=-1,tick=0,lease,activeFault='';
  const view=()=>({actor:1,controller:'native',paused,position:{x:1,y:1},session:{world,player:1},world:world===-1?'Tutorial':'Nexus',lifecycle:{alive:true},character:{hp:100,mp:50,items:[]},verification:{shots:0}});
  let origin;
  const server=createServer(async(req,res)=>{
    res.setHeader('Content-Type','application/json');
    if(req.url==='/json/list')return res.end(JSON.stringify([{id:'owned-fixture',url:origin+'/',webSocketDebuggerUrl:'ws://127.0.0.1/fixture'}]));
    assert.equal(req.headers.authorization,'Bearer fixture-control');
    const chunks=[];for await(const chunk of req)chunks.push(chunk);
    const request=JSON.parse(Buffer.concat(chunks));calls.push(request);
    const c=request.command;
    if(c.op==='native_state')return res.end(JSON.stringify({runtime:'fixture-runtime',paused,actors:[{actor:1,player:1,stage:activeFault==='loading'?'disconnected':'ready',world,world_name:world===-1?'Tutorial':'Nexus',client_time:++tick}]}));
    if(c.op==='native_sync'){
      if(activeFault==='binding-renew-timeout'&&!paused)await new Promise(resolve=>setTimeout(resolve,900));
      return res.end(JSON.stringify({acknowledged:[{actor:1,player:1,frame_sha256:createHash('sha256').update(activeFault==='binding'?Buffer.from('wrong-frame'):Buffer.from('000000090600000001','hex')).digest('hex')}]}));
    }
    if(c.op==='native_lease'&&c.operation==='renew'&&['binding-renew-timeout','layout-renew-timeout'].includes(activeFault)){
      await new Promise(resolve=>setTimeout(resolve,1000));
      return res.end(JSON.stringify({status:'failed',error:'fixture delayed renewal'}));
    }
    if(c.op==='native_lease'&&c.operation==='renew'&&activeFault==='terminal'){paused=true;return res.end(JSON.stringify({status:'failed',error:'native participant is terminal during control'}));}
    if(c.op==='native_lease'){clearTimeout(lease);if(c.operation!=='end')lease=setTimeout(()=>{paused=true;},1500);}
    if(c.op==='pause'||c.op==='native_lease'&&c.operation==='halt')paused=true;if(c.op==='resume'){paused=false;if(transfer)world=-2;}
    res.end(JSON.stringify(c.op==='observe'?view():c.op==='run'?{status:'succeeded',result:{goal_reached:true,state:view()}}:{status:'succeeded',paused}));
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const port=server.address().port;origin=`http://127.0.0.1:${port}`;
  const helper=join(dir,'managed-fixture.mjs'), bootstrap=join(dir,'cdp-fixture.mjs'), log=join(dir,'cdp.jsonl');
  await writeFile(helper,"export const scriptOwner=()=> 'synthetic-test-owner'; export const managedCDP=async(port,target,owner,send)=>send;");
  await writeFile(bootstrap,`
    import {appendFileSync} from 'node:fs';
    globalThis.WebSocket=class {
      constructor(){this.binding=null;queueMicrotask(()=>this.onopen());}
      send(raw){const row=JSON.parse(raw);appendFileSync(process.env.FIXTURE_LOG,JSON.stringify({...row,at:Date.now()})+'\\n');
        let value=true,error;
        if(row.method==='Network.enable')setTimeout(()=>{const frame=payloadData=>this.onmessage({data:JSON.stringify({method:'Network.webSocketFrameReceived',params:{response:{opcode:2,payloadData}}})});frame(Buffer.from('000000090600000001','hex').toString('base64'));if(process.env.FIXTURE_FAULT==='combat-frames')for(let n=0;n<256;n++)frame(Buffer.from('000000094a00000001','hex').toString('base64'));},0);
        if(row.method==='Runtime.evaluate'&&row.params.expression.includes('playing:'))value={playing:true,binding:this.binding};
        if(row.method==='Runtime.evaluate'&&row.params.expression.startsWith('globalThis[Symbol.for("openrotmg.testing.binding.v1")]=')){this.binding=JSON.parse(row.params.expression.split(']=')[1]);value=this.binding;}
        if(row.method==='Runtime.evaluate'&&row.params.expression.endsWith(')(null)'))value={initialized:false,paused:true,clock_ms:100};
        if(row.method==='Runtime.evaluate'&&row.params.expression.includes('getBoundingClientRect'))value={x:0,y:0,width:800,height:600,viewportWidth:800,viewportHeight:600};
        if(process.env.FIXTURE_FAULT==='input'&&row.method==='Input.dispatchKeyEvent'&&row.params.type==='keyDown')error={message:'fixture keydown failure'};
        if(process.env.FIXTURE_FAULT==='pause'&&row.method==='Runtime.evaluate'&&row.params.expression.endsWith(')(true)'))error={message:'fixture native pause failure'};
        setTimeout(()=>this.onmessage({data:JSON.stringify({id:row.id,...(error?{error}:{result:row.method==='Runtime.evaluate'?{result:{value}}:{}})})}),process.env.FIXTURE_FAULT==='slow-ack'&&row.method==='Input.dispatchKeyEvent'&&row.params.type==='keyDown'?1000:process.env.FIXTURE_FAULT==='layout-renew-timeout'&&row.method==='Runtime.evaluate'&&row.params.expression.includes('getBoundingClientRect')?900:0);
      }
      close(){this.onclose?.();}
    };
  `);
  const config=join(dir,'config.json');
  await writeFile(config,JSON.stringify({origin,state:dir,browserTools:helper,native:[{port,target:'owned-fixture',actor:1}]}));
  t.after(async()=>{clearTimeout(lease);await new Promise(resolve=>server.close(resolve));await rm(dir,{recursive:true});});
  async function run(id,command,fault='') {
    activeFault=fault;
    const path=join(dir,'command.json');await writeFile(path,JSON.stringify({id,command}));
    try {return {...await exec(process.execPath,['--import',bootstrap,script,config,path],{env:{...process.env,FIXTURE_LOG:log,FIXTURE_FAULT:fault},timeout:20000}),code:0};}
    catch(e){return {stdout:e.stdout,stderr:e.stderr,code:e.code};}
  }
  const events=async()=>{try{return(await readFile(log,'utf8')).trim().split('\n').filter(Boolean).map(JSON.parse);}catch(e){if(e.code==='ENOENT')return[];throw e;}};
  return {dir,calls,run,events,paused:()=>paused};
}
const move={op:'act',actor:1,input:{keys:['d'],duration_ms:1},pauseAfter:true};
test('native request releases keys, coordinates pause, and replays without repeated effects',async t=>{
  const f=await fixture(t), first=await f.run('move-once',move);
  assert.equal(first.code,0,first.stderr);
  assert.equal(JSON.parse(first.stdout).pause_scope,'server_and_native');
  const events=await f.events();
  assert.deepEqual(events.filter(e=>e.method==='Input.dispatchKeyEvent').map(e=>e.params.type),['keyDown','keyUp']);
  assert.ok(!events.some(e=>/bringToFront|activateTarget|setWindowBounds/.test(e.method)));
  const before=f.calls.length, second=await f.run('move-once',move);
  assert.equal(second.code,0,second.stderr);
  assert.equal(f.calls.length,before);
  assert.equal((await f.events()).length,events.length);
  assert.equal(f.paused(),true);
});
test('cross-world coordinates never report a physical movement distance',async t=>{
  const f=await fixture(t,true),result=await f.run('ordinary-transfer',move);
  assert.equal(result.code,0,result.stderr);
  const effects=JSON.parse(result.stdout).effects;
  assert.equal(effects.world_changed,true);assert.equal(effects.distance,null);
});
test('primary input failure retains release and pause cleanup evidence',async t=>{
  const f=await fixture(t), result=await f.run('bad-input',move,'input');
  assert.equal(result.code,1);
  assert.match(result.stderr,/fixture keydown failure/);
  assert.ok((await f.events()).some(e=>e.method==='Input.dispatchKeyEvent'&&e.params.type==='keyUp'));
  assert.ok(f.calls.some(e=>e.command.op==='pause'));
  assert.equal(f.paused(),true);
});
test('unconfirmed native pause never disarms its independent expiry guard' ,async t=>{
  const f=await fixture(t), result=await f.run('bad-pause',move,'pause');
  assert.equal(result.code,1);
  assert.match(result.stderr,/fixture native pause failure/);
  assert.ok(!f.calls.some(e=>e.command.op==='native_lease'&&e.command.operation==='end'));
  await new Promise(r=>setTimeout(r,1600));
  assert.equal(f.paused(),true);
});
test('unknown input and changed id reuse fail without resuming',async t=>{
  const f=await fixture(t), result=await f.run('bad-fields',{...move,input:{...move.input,immunity:true}});
  assert.equal(result.code,1);
  assert.ok(!f.calls.some(e=>e.command.op==='resume'));
  const first=await f.run('valid',move);assert.equal(first.code,0,first.stderr);
  const before=f.calls.length, conflict=await f.run('valid',{...move,pauseAfter:false});
  assert.equal(conflict.code,1);assert.match(conflict.stderr,/different command/);assert.equal(f.calls.length,before);
});
test('real-controller wrapper retains stable run id and refreshes post-pause state',async t=>{
  const f=await fixture(t), result=await f.run('bounded-goal',{op:'run',actor:1,goal:{kind:'time',ms:100},max_game_ms:1000});
  assert.equal(result.code,0,result.stderr);
  assert.equal(f.calls.find(e=>e.command.op==='run').id,'bounded-goal');
  assert.equal(JSON.parse(result.stdout).result.state.paused,true);
});

test('disconnected participant cannot initialize native clocks or report pause success',async t=>{
  const f=await fixture(t),result=await f.run('loading',{op:'pause'},'loading');
  assert.equal(result.code,1);assert.match(result.stderr,/readiness/);
  assert.ok(!(await f.events()).some(e=>e.method==='Runtime.evaluate'&&e.params.expression.endsWith(')(null)')));
  assert.ok(!f.calls.some(c=>['pause','resume'].includes(c.command.op)));
});
test('swapped native browser identity is rejected before any input or resume',async t=>{
  const f=await fixture(t),result=await f.run('binding',move,'binding');
  assert.equal(result.code,1);assert.match(result.stderr,/binding mismatch/);
  assert.ok(!(await f.events()).some(e=>e.method.startsWith('Input.')));
  assert.ok(!f.calls.some(c=>c.command.op==='resume'));
});
test('UP deadline starts at actual DOWN dispatch, not a slow acknowledgement',async t=>{
  const f=await fixture(t),result=await f.run('slow-ack',{...move,input:{keys:['d'],duration_ms:150}},'slow-ack');
  assert.equal(result.code,0,result.stderr);
  const keys=(await f.events()).filter(e=>e.method==='Input.dispatchKeyEvent');
  assert.deepEqual(keys.map(e=>e.params.type),['keyDown','keyUp']);
  assert.ok(keys[1].at-keys[0].at>=140&&keys[1].at-keys[0].at<250,JSON.stringify(keys));
});
test('terminal cohort supervision releases an active hold before its duration expires',async t=>{
  const f=await fixture(t),result=await f.run('terminal-during-hold',{...move,input:{keys:['d'],duration_ms:4000}},'terminal');
  assert.equal(result.code,1);assert.match(result.stderr,/terminal during control/);
  const keys=(await f.events()).filter(e=>e.method==='Input.dispatchKeyEvent');
  assert.deepEqual(keys.map(e=>e.params.type),['keyDown','keyUp']);
  assert.ok(keys[1].at-keys[0].at<1000,JSON.stringify(keys));
  assert.equal(f.paused(),true);
});
for(const [name,input] of [['key',{keys:['d'],duration_ms:1}],['mouse',{keys:[],duration_ms:100,drag:{from:{x:634,y:528},to:{x:722,y:428}}}]])test(`failed renewal during binding cannot start a late ${name} DOWN`,async t=>{
  const f=await fixture(t),result=await f.run('failed-binding-renew',{...move,input},'binding-renew-timeout');
  assert.equal(result.code,1);assert.match(result.stderr,/TimeoutError/);
  const down=(await f.events()).filter(e=>e.method.startsWith('Input.')&&['keyDown','mousePressed'].includes(e.params.type));
  assert.deepEqual(down,[],'A timed-out lease must reject new input, even when its browser proof arrives later');
  assert.ok(f.calls.some(c=>c.command.op==='native_lease'&&c.command.operation==='halt'));
  assert.ok(!f.calls.some(c=>c.command.op==='native_lease'&&c.command.operation==='end'));
  assert.equal(f.paused(),true);
});
test('failed renewal during binding cannot dispatch a server run',async t=>{
  const f=await fixture(t),result=await f.run('failed-binding-run',{op:'run',actor:1,goal:{kind:'time',ms:100},max_game_ms:1000},'binding-renew-timeout');
  assert.equal(result.code,1);assert.match(result.stderr,/TimeoutError/);
  assert.ok(!f.calls.some(c=>c.command.op==='run'),'A late browser proof cannot authorize server work after heartbeat failure');
  assert.ok(f.calls.some(c=>c.command.op==='native_lease'&&c.command.operation==='halt'));
});
test('failed renewal during layout cannot dispatch a mouse DOWN',async t=>{
  const f=await fixture(t),result=await f.run('failed-layout-renew',{...move,input:{keys:[],duration_ms:100,mouse:{x:450,y:300}}},'layout-renew-timeout');
  assert.equal(result.code,1);assert.match(result.stderr,/TimeoutError/);
  assert.ok(!(await f.events()).some(e=>e.method==='Input.dispatchMouseEvent'&&e.params.type==='mousePressed'));
  assert.ok(f.calls.some(c=>c.command.op==='native_lease'&&c.command.operation==='halt'));
});
test('native realtime work cannot outlive a successful standalone controller',async t=>{
  const f=await fixture(t);
  const a=await f.run('no-orphan',{...move,pauseAfter:false}),b=await f.run('no-resume',{op:'resume'});
  assert.equal(a.code,1);assert.equal(b.code,1);
  assert.ok(!f.calls.some(c=>c.command.op==='resume'));
});

test('combat frames cannot evict a pending native binding proof',async t=>{
  const f=await fixture(t),result=await f.run('combat-proof',move,'combat-frames');
  assert.equal(result.code,0,result.stderr);
});
test('bounded native loot drag dispatches actual down, move and up',async t=>{
  const f=await fixture(t),result=await f.run('loot-drag',{op:'act',actor:1,input:{keys:[],duration_ms:200,drag:{from:{x:634,y:528},to:{x:722,y:428}}}});
  assert.equal(result.code,0,result.stderr);
  const events=(await f.events()).filter(e=>e.method==='Input.dispatchMouseEvent');
  assert.deepEqual(events.map(e=>e.params.type),['mouseMoved','mousePressed','mouseMoved','mouseReleased']);
  assert.deepEqual(events.map(e=>[e.params.x,e.params.y]),[[634,528],[634,528],[722,428],[722,428]]);
  assert.ok(events[3].at-events[1].at>=190&&events[3].at-events[1].at<350);
});
test('invalid native drags reject before resuming or dispatching input',async t=>{
  const f=await fixture(t),result=await f.run('invalid-drag',{op:'act',actor:1,input:{keys:[],duration_ms:200,drag:{from:{x:634,y:528},to:{x:800,y:428}}}});
  assert.equal(result.code,1);assert.match(result.stderr,/Native drag requires/);
  assert.ok(!f.calls.some(c=>c.command.op==='resume'));
  assert.ok(!(await f.events()).some(e=>e.method.startsWith('Input.')));
});
