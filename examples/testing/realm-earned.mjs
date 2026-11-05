import assert from 'node:assert/strict';
import {readFile,writeFile,readdir,realpath,stat} from 'node:fs/promises';
import {join} from 'node:path';
import {createHash,randomUUID} from 'node:crypto';
import {connect} from '../../scripts/record-demo.mjs';
import {earnedLedger,collisionMap,route,originalIdentity,localTargets,textReceiver,assertCastleArrival,structuredEvents,mapInput,readMap} from './realm-earned-core.mjs';

export const realmSeed=1344;
export const fixture={class:0x310,level:20,seed:1344,world:-2,position:{x:78.5,y:42.5},equipment:[0xaf6,0xb25,0xb05,0xac5],backpack:Array(8).fill(-1),stats:[670,385,50,25,50,50,40,75]};
const profiles=['DrFlux','GHZD','DrtySausge','Kryz','TKGP','HighTech','TheDezaq','Nutzz','RobbyHobby','FnSjeZZe','Cronus','Demoted'];
const auxiliary=(profile,i)=>({class:0x307,level:20,seed:1346+i,equipment:[0xc02,0xb28,0xaf9,0xac5],backpack:Array(8).fill(-1),stats:[700,252,75,25,50,50,40,50],controller:'game_bot',profile});
const roots=[0x91f,0x91b,0x928,0x675,0x678,0xd56,0xd59,0xd5f,0xd50,0xd54,0xd61,0xe37],heroes=[0x668,0x67d],distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y),wire=[];
const caps={wallMs:2700000,cleanupMs:120000,activeMs:2400000,combatMs:2200000,acts:600,combatActs:530,receipts:1000,combatReceiptReserve:100,tailReserveMs:190000};
export async function beforeNavigate({browser,signal}){
  const receiver=textReceiver(),cdp=await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:e=>receiver.event(e)});
  signal.addEventListener('abort',()=>cdp.close(),{once:true});wire.push({browser,receiver});await cdp.call('Network.enable');
}
export async function run(fighter){
  const out=process.env.QA_RESULTS_DIR,native=join(out,'native'),configFile=join(native,'native.json'),config=JSON.parse(await readFile(configFile,'utf8')),registry=JSON.parse(await readFile(join(native,'children.json'),'utf8'));
  assert.equal(await realpath(config.state),join(await realpath(native),'state'));assert.equal(registry.parent.pid,process.pid);assert(registry.deadline-Date.now()<=caps.wallMs);
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),headers={Authorization:'Bearer '+token,'Content-Type':'application/json'};let serial=0,acts=0,tail=false,ledger=null,consumed='',events=[],lastGame=-1,lastShared=-1;
  const api=async command=>{
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:'earned-'+(++serial),command}),signal:AbortSignal.timeout(10000)}),body=await response.json();
    assert(response.ok&&body.status!=='failed',JSON.stringify(body));
    if(['capabilities','world_info','observe','native_state'].includes(command.op)){assert(!Object.hasOwn(body,'result'),'Read response schema changed');return body;}
    assert.equal(body.status,'succeeded');return body.result;
  };
  const parseEvent=structuredEvents(config.origin);
  const logs=async()=>{
    const text=await readFile(join(native,'server.log'),'utf8'),complete=text.slice(0,text.lastIndexOf('\n')+1);assert(complete.startsWith(consumed),'Owned log prefix changed');
    for(const line of complete.slice(consumed.length).split('\n')){const fields=parseEvent(line);if(fields!==null){events.push(fields);ledger?.ingest(fields);}}consumed=complete;if(ledger)await writeFile(join(out,'earned-ledger-latest.json'),JSON.stringify(ledger.snapshot()));return events;
  };
  const map=async(client,summary)=>readMap(config.origin,headers,(await client.observe()).session.player,summary);
  const observer=await fighter.join({...fixture,seed:1345}),first=await fighter.observe(),observerFirst=await observer.observe(),capability=await api({op:'capabilities'}),bootstrap=JSON.parse(await readFile(configFile,'utf8')).native_bootstrap;
  assert.equal(capability.mutation_limit,1024);assert(capability.controllers.includes('game_bot')&&profiles.every(p=>capability.game_bot_profiles.includes(p)));assert.equal(wire.length,2);
  const originals=[originalIdentity('fighter',fixture,first),originalIdentity('observer',{...fixture,seed:1345},observerFirst)],samples=[];
  const save=async name=>{const row={name,fighter:await fighter.inspect(),observer:await observer.inspect(),ledger:ledger?.snapshot()};samples.push(row);await writeFile(join(out,'earned-journey.json'),JSON.stringify(samples));await writeFile(join(out,'earned-text.json'),JSON.stringify(wire.map(w=>({browser:w.browser,texts:w.receiver.texts,error:w.receiver.error}))));return row.fighter.view;};
  const budget=async duration=>{
    const state=await api({op:'native_state',actors:[fighter.actor,observer.actor]}),shared=state.shared_active_time,clock=state.actors.find(a=>a.actor===observer.actor).observation,receipts=(await readdir(join(config.state,'testing'))).filter(n=>n.endsWith('.receipt.json')).length;
    assert(state.native_state_version===1&&state.runtime===bootstrap.runtime&&shared.version===1&&shared.runtime===bootstrap.runtime&&shared.epoch==='native_control_binding');
    assert(clock.world==='Nexus'&&clock.session.world===-2&&clock.lifecycle.alive&&Number.isSafeInteger(clock.game_ms)&&clock.game_ms>=lastGame&&Number.isSafeInteger(shared.elapsed_ms)&&shared.elapsed_ms>=lastShared,'Original budget clock changed or regressed');lastGame=clock.game_ms;lastShared=shared.elapsed_ms;
    assert(acts<caps.acts&&(tail||acts<caps.combatActs));assert(receipts+(tail?0:caps.combatReceiptReserve)<caps.receipts,'Actual receipt reserve exhausted');
    assert(clock.game_ms+duration<=caps.activeMs&&shared.elapsed_ms+duration<=caps.activeMs&&(tail||clock.game_ms+duration<=caps.combatMs&&shared.elapsed_ms+duration<=caps.combatMs),'Actual clock reserve exhausted');
    assert(registry.deadline-Date.now()-caps.cleanupMs>=(tail?duration+10000:caps.tailReserveMs),'Original whole-wall reserve exhausted');
    for(const w of wire)assert.equal(w.receiver.error,null,'Native text decoder failed');
    await writeFile(join(out,'earned-budget.json'),JSON.stringify({caps,acts,tail,gameMs:clock.game_ms,sharedActiveMs:shared.elapsed_ms,receipts,deadline:registry.deadline,wallNow:Date.now()}));
  };
  const act=async(client,input,shot)=>{
    assert(Number.isSafeInteger(input.duration_ms)&&input.duration_ms>0&&input.duration_ms<=10000);await budget(input.duration_ms);acts++;
    const before=await client.observe(),result=await client.act(input,shot?{name:shot,when:v=>v.session.world===before.session.world&&v.game_ms>=before.game_ms+600}:undefined);
    if(shot){const mtime=(await stat(join(native,shot+'.png'))).mtimeMs,step=result.steps.find(s=>s.actor===client.actor);assert(step&&mtime>=step.input_started_wall_ms&&mtime<=step.input_release_wall_ms,'Native image write was not inside the actual held-input interval');}
    await logs();return result;
  };
  const walk=async(client,goal)=>{
    const world=(await client.observe()).session.world;
    for(let n=0;n<80;n++){
      const v=await client.observe();if(v.session.world!==world)return;
      if(client===fighter&&world!==-2&&ledger?.snapshot().pending.length===0)return;
      const dx=goal.x-v.position.x,dy=goal.y-v.position.y,d=distance(goal,v.position);if(d<.6&&Math.floor(goal.x)===Math.floor(v.position.x)&&Math.floor(goal.y)===Math.floor(v.position.y))return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];if(Math.abs(x)>.12)keys.push(x>0?'d':'a');if(Math.abs(y)>.12)keys.push(y>0?'s':'w');
      await act(client,{keys,duration_ms:Math.min(6000,Math.max(80,Math.round(d*1000/7.734)))});
    }throw Error('Collision-legal waypoint budget exhausted');
  };
  const enter=async(client,portal)=>{await walk(client,portal.position);await act(client,{keys:[],mouse:{x:700,y:575},duration_ms:1000});assert.equal((await client.observe()).world,'Realm');};
  const nexus=(await observer.inspect()).view.objects.filter(o=>o.kind===0x712);assert.equal(nexus.length,2);const entrance=nexus[0],other=nexus[1];await walk(observer,other.position);await enter(fighter,entrance);
  const entered=await fighter.observe(),realm=entered.session.world,info=await api({op:'world_info',world:realm});let terrain=await map(fighter,true);
  const initial=terrain.entities.filter(e=>heroes.includes(e.kind)||roots.includes(e.kind)&&e.quest===e.id);ledger=earnedLedger(realm,initial.map(e=>e.id),events);await logs();assert(ledger.snapshot().census&&ledger.snapshot().credited.length===0,'Unreduced initial roster must precede combat');
  const selection=mapInput(info.realm_layout,JSON.parse(await readFile('data/realm-catalog.json','utf8')));let file;
  if(selection.recipe){const recipe=JSON.parse(await readFile(join('data/realms',selection.recipe),'utf8'));assert.equal(recipe.id,info.realm_layout);file=join('data/realms',recipe.output);}else file=selection.file;
  const raw=await readFile(file);assert.equal(createHash('sha256').update(raw).digest('hex'),selection.sha256);let collision=collisionMap(terrain);
  await writeFile(join(out,'earned-source.json'),JSON.stringify({scope:'Prepared distributed raid, ordinary unreduced quest/event combat; not coast/level1/history/population acceptance. Auxiliary initial positions are declared before their own enrollment. No existing actor relocation, protection, direct kill, progress injection or replacement characters.',runtime:bootstrap.runtime,source:JSON.parse(await readFile(join(native,'result.json'),'utf8')).source,initialRoots:initial,map:{id:info.realm_layout,file,sha256:createHash('sha256').update(raw).digest('hex')},caps}));
  const starts=[{position:entered.position}],chosen=new Set();
  for(let i=1;i<profiles.length;i++){
    const target=initial.filter(e=>!chosen.has(e.id)).sort((a,b)=>Math.min(...starts.map(p=>distance(b.pos,p.position)))-Math.min(...starts.map(p=>distance(a.pos,p.position)))||a.id-b.id)[0];assert(target);chosen.add(target.id);const positions=[];
    for(let dy=-9;dy<=9;dy++)for(let dx=-9;dx<=9;dx++){const p={x:Math.floor(target.pos.x)+dx+.5,y:Math.floor(target.pos.y)+dy+.5},d=distance(p,target.pos);if(p.x>=0&&p.y>=0&&p.x<collision.width&&p.y<collision.height&&d>=6&&d<=9&&!collision.blocked[Math.floor(p.y)*collision.width+Math.floor(p.x)]&&starts.every(s=>distance(s.position,p)>=1))positions.push(p);}
    positions.sort((a,b)=>distance(a,entered.position)-distance(b,entered.position)||a.y-b.y||a.x-b.x);assert(positions.length,'No legal prepared front');starts.push({position:positions[0],root:target.id});
  }
  const cohort=randomUUID();for(let i=0;i<profiles.length;i++){
    const spec={guid:'earned-'+cohort+'-'+i,...auxiliary(profiles[i],i),world:realm,position:starts[i].position},created=await api({op:'spawn',spec}),v=await api({op:'observe',actor:created.actor});assert(created.connected&&created.controller==='game_bot'&&created.world===realm&&v.registered_profile===profiles[i]);originals.push(originalIdentity(profiles[i],spec,v));await writeFile(join(out,'earned-originals.json'),JSON.stringify({runtime:bootstrap.runtime,originals,fronts:starts}));
  }
  assert(originals.length===14&&new Set(originals.map(o=>o.account)).size===14&&new Set(originals.map(o=>o.character)).size===14);let invitation=-Infinity;
  await save('initial-party');
  for(let iteration=0;iteration<300;iteration++){
    await logs();if(ledger.snapshot().pending.length===0)break;
    const v=await fighter.observe();assert(v.lifecycle.alive&&v.session.world===realm,'Original fighter left combat before earned closure');
    if((await observer.observe()).game_ms-invitation>=45000){await act(fighter,{keys:[],duration_ms:1000,chat:'DrFlux follow me'});invitation=(await observer.observe()).game_ms;}
    terrain=await map(fighter,true);collision=collisionMap(terrain);const pending=new Set(ledger.snapshot().pending),family=terrain.entities.filter(e=>(pending.has(e.id)||pending.has(e.quest)||pending.has(e.parent))&&e.hp>0).sort((a,b)=>distance(a.pos,v.position)-distance(b.pos,v.position));
    if(!family.length){assert(ledger.snapshot().waiting.length>0,'Pending ledger has neither a living family nor an actual waiting event');await act(fighter,{keys:[],duration_ms:3000});continue;}
    const view=(await fighter.inspect()).view,visible=localTargets(view.objects,family,v.position),target=visible[0];
    if(!target){const points=route(collision,v.position,family[0].pos);assert(points.length>1,'Nearby pending family is absent from the native neighborhood');await walk(fighter,points[1]);}
    else{const heal=v.character.hp<v.character.stats.max_hp*.8&&v.character.mp>=85;await act(fighter,{keys:heal?[' ']:[],duration_ms:1500,target:{entity:target.id,camera_angle:-Math.PI/4,fire:true}});}
    if(iteration%10===0)await save('combat-'+iteration);
  }
  await logs();const completed=ledger.complete();tail=true;await writeFile(join(out,'earned-ledger.json'),JSON.stringify(completed));
  assert(!(await observer.inspect()).view.objects.some(o=>o.id===entrance.id),'Earned closure must remove its exact entrance');
  const closing='I HAVE CLOSED THIS REALM! YOU WILL NOT LIVE TO SEE THE LIGHT OF DAY!';assert(wire[0].receiver.texts.some(t=>t.name==='#Oryx the Mad God'&&t.text===closing),'Actual fighter did not receive closure TEXT');
  await act(fighter,{keys:[],duration_ms:3000},'earned-closed-realm');
  for(let n=0;n<14&&(await fighter.observe()).session.world===realm;n++)await act(fighter,{keys:[],duration_ms:10000});
  const castle=await save('earned-Castle');assertCastleArrival(castle,first);
  await logs();const sealed=events.filter(e=>e.origin===realm&&e.message==='Realm closure deadline reached'&&typeof e.stage==='string'&&e.stage.startsWith('Sealed')),transfers=events.filter(e=>e.origin===realm&&e.message==='Realm closure deadline reached'&&e.stage==='Transferring');assert.equal(sealed.length,1);assert.equal(transfers.length,1);assert.equal(sealed[0].stage,`Sealed { deadline: ${completed.last.now+128000} }`);assert(sealed[0].now>=completed.last.now+120000&&transfers[0].now>=completed.last.now+128000);
  for(const text of ['MY MINIONS HAVE FAILED ME!','BUT NOW YOU SHALL FEEL MY WRATH','COME MEET YOUR DOOM AT THE WALLS OF MY CASTLE!'])assert(wire[0].receiver.texts.some(t=>t.name==='#Oryx the Mad God'&&t.text===text),'Missing actual closure TEXT '+text);
  for(let n=0;n<8&&!events.some(e=>e.origin===realm&&e.message==='safe collective departure completed; replacement published');n++)await act(fighter,{keys:[],duration_ms:500});
  const publication=events.find(e=>e.origin===realm&&e.message==='safe collective departure completed; replacement published'),departure=events.find(e=>e.origin===realm&&e.message==='all collective recipients durably departed; replacement delay started');assert(publication&&departure&&publication.directory_tick>=departure.publish_tick,'Exact Directory-gated publication log missing');
  const handoff=events.find(e=>e.origin===realm&&e.message==='Realm closed; collective Castle route and fresh Realm installed');assert(handoff&&Number.isSafeInteger(handoff.replacement)&&castle.session.world===handoff.castle);
  const nexusMap=await map(observer,false),fresh=nexusMap.entities.filter(e=>e.kind===0x712&&e.id!==other.id&&e.id!==entrance.id);assert.equal(fresh.length,1,'Exact fresh portal must be unambiguous');await walk(observer,fresh[0].pos);
  const portals=(await observer.inspect()).view.objects.filter(o=>o.kind===0x712),successor=portals.find(o=>o.id===fresh[0].id);assert(successor&&successor.usable&&successor.id!==entrance.id&&nexusMap.entities.some(o=>o.id===other.id));
  await act(observer,{keys:[],duration_ms:3000},'earned-published-successor');await act(fighter,{keys:['Insert'],duration_ms:1500});assert.equal((await fighter.observe()).world,'Nexus');await enter(fighter,successor);assert.equal((await fighter.observe()).session.world,handoff.replacement);await act(fighter,{keys:['Insert'],duration_ms:1500});
  const final=await save('final');for(const [before,after]of [[first,final],[observerFirst,await observer.observe()]]){assert(after.lifecycle.alive);assert.equal(after.session.character,before.session.character);assert.equal(after.session.account,before.session.account);assert.deepEqual(after.character.items,before.character.items);assert.equal(after.character.inventory_revision,before.character.inventory_revision);}
  await writeFile(join(out,'earned-handoff.json'),JSON.stringify({handoff,departure,publication,qualification:'Actual source-conditioned Directory deadline/order, not an independently measured departure interval, forced negative-pending probe, every-recipient or restart-durability proof.'}));
  fighter.check('unreduced normal census and every admitted event earned; original native receives notices and Castle; exact successor entered with durable original inventory',completed.pending.length===0&&completed.admitted.length>0&&final.lifecycle.alive&&castle.session.character===first.session.character&&successor.id===fresh[0].id);
  await writeFile(join(out,'earned-subchecks.json'),JSON.stringify({functional:'PASS',earned:completed,originalNativeSurvival:'PASS',auxiliaryOutcomes:'REQUIRES_OWN_TERMINAL_COLD_AUDIT',nativeCameraShake:'NOT_CLAIMED',historical:'NOT_CLAIMED',cohort}));
}
