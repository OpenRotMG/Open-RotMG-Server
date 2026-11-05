import assert from 'node:assert/strict';
import {readFile,writeFile,appendFile,readdir,realpath,stat} from 'node:fs/promises';
import {join} from 'node:path';
import {createHash,randomUUID} from 'node:crypto';
import {connect} from '../../scripts/record-demo.mjs';
import {earnedLedger,collisionMap,route,originalIdentity,localTargets,textReceiver,assertCastleArrival,structuredEvents,mapInput,readMap,recoveryState,replacementEntrance,otherEntrance,observedAim,inputWork,nativeReceiptRoom,partyPositions,rejoinMarker,walkingKeys} from './realm-earned-core.mjs';

export const realmSeed=1344;
export const fixture={class:0x310,level:20,seed:1344,world:-2,position:{x:78.5,y:42.5},equipment:[0xaf6,0xb25,0xb05,0xac5],backpack:Array(8).fill(-1),stats:[670,385,50,25,50,50,40,75]};
const profiles=['DrFlux','GHZD','DrtySausge','Kryz','TKGP','HighTech','TheDezaq','Nutzz','RobbyHobby','FnSjeZZe','Cronus','Demoted'];
const auxiliary=(profile,i)=>({class:0x307,level:20,seed:1346+i,equipment:[0xc02,0xb28,0xaf9,0xac5],backpack:Array(8).fill(-1),stats:[700,252,75,25,50,50,40,50],controller:'game_bot',profile});
const roots=[0x91f,0x91b,0x928,0x675,0x678,0xd56,0xd59,0xd5f,0xd50,0xd54,0xd61,0xe37],heroes=[0x668,0x67d],distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y),wire=[];
const caps={wallMs:2700000,cleanupMs:120000,activeMs:2400000,combatMs:2200000,inputs:600,combatInputs:530,nativeReceipts:240,combatNativeReserve:50,receipts:1000,combatReceiptReserve:100,tailReserveMs:190000};
export async function beforeNavigate({browser,signal}){
  const receiver=textReceiver(),cdp=await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:e=>receiver.event(e)});
  signal.addEventListener('abort',()=>cdp.close(),{once:true});wire.push({browser,receiver});await cdp.call('Network.enable');
}
export async function run(fighter){
  const out=process.env.QA_RESULTS_DIR,native=join(out,'native'),configFile=join(native,'native.json'),config=JSON.parse(await readFile(configFile,'utf8')),registry=JSON.parse(await readFile(join(native,'children.json'),'utf8'));
  assert.equal(await realpath(config.state),join(await realpath(native),'state'));assert.equal(registry.parent.pid,process.pid);assert(registry.deadline-Date.now()<=caps.wallMs);
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),headers={Authorization:'Bearer '+token,'Content-Type':'application/json'};let serial=0,acts=0,inputs=0,invitation=-Infinity,tail=false,ledger=null,consumed='',events=[],lastGame=-1,lastShared=-1;
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
  const budget=async({duration,inputs:planned})=>{
    const state=await api({op:'native_state',actors:[fighter.actor,observer.actor]}),shared=state.shared_active_time,clock=state.actors.find(a=>a.actor===observer.actor).observation,files=await readdir(join(config.state,'testing')),receipts=files.filter(n=>n.endsWith('.receipt.json')).length,nativeReceipts=nativeReceiptRoom(files,caps.nativeReceipts,tail?0:caps.combatNativeReserve);
    assert(state.native_state_version===1&&state.runtime===bootstrap.runtime&&shared.version===1&&shared.runtime===bootstrap.runtime&&shared.epoch==='native_control_binding');
    assert(clock.world==='Nexus'&&clock.session.world===-2&&clock.lifecycle.alive&&Number.isSafeInteger(clock.game_ms)&&clock.game_ms>=lastGame&&Number.isSafeInteger(shared.elapsed_ms)&&shared.elapsed_ms>=lastShared,'Original budget clock changed or regressed');lastGame=clock.game_ms;lastShared=shared.elapsed_ms;
    assert(inputs+planned<=caps.inputs&&(tail||inputs+planned<=caps.combatInputs),'Planned raw-input reserve exhausted');assert(receipts+(tail?0:caps.combatReceiptReserve)<caps.receipts,'Actual receipt reserve exhausted');
    assert(clock.game_ms+duration<=caps.activeMs&&shared.elapsed_ms+duration<=caps.activeMs&&(tail||clock.game_ms+duration<=caps.combatMs&&shared.elapsed_ms+duration<=caps.combatMs),'Actual clock reserve exhausted');
    assert(registry.deadline-Date.now()-caps.cleanupMs>=(tail?duration+10000:caps.tailReserveMs),'Original whole-wall reserve exhausted');
    for(const w of wire)assert.equal(w.receiver.error,null,'Native text decoder failed');
    await writeFile(join(out,'earned-budget.json'),JSON.stringify({caps,acts,inputs,plannedInputs:planned,nativeReceipts,tail,gameMs:clock.game_ms,sharedActiveMs:shared.elapsed_ms,receipts,deadline:registry.deadline,wallNow:Date.now()}));
  };
  const act=async(client,input,shot)=>{
    const work=inputWork(input,client.actor);await budget(work);acts++;inputs+=work.inputs;
    const before=await client.observe(),result=await client.act(input,shot?{name:shot,when:v=>v.session.world===before.session.world&&v.game_ms>=before.game_ms+600}:undefined);
    if(shot){const mtime=(await stat(join(native,shot+'.png'))).mtimeMs,step=result.steps.find(s=>s.actor===client.actor);assert(step&&mtime>=step.input_started_wall_ms&&mtime<=step.input_release_wall_ms,'Native image write was not inside the actual held-input interval');}
    await logs();return result;
  };
  const invite=async()=>{
    if(tail||originals.length!==14||(await observer.observe()).game_ms-invitation<45000)return;
    const v=await fighter.observe();if(v.world!=='Realm'||recoveryState(v.character).retreat)return;
    const view=(await fighter.inspect()).view,visible=view.objects.filter(o=>profiles.includes(o.name)&&o.hp>0),sequence=visible.map(o=>({actor:fighter.actor,input:{keys:[],duration_ms:1100,chat:o.name+' follow me'}}));assert(new Set(visible.map(o=>o.name)).size===visible.length);
    await appendFile(join(out,'earned-calls.jsonl'),JSON.stringify({gameMs:v.game_ms,actor:fighter.actor,visible:visible.map(o=>({id:o.id,name:o.name,position:o.position})),claim:'planned named local invitations only; no acceptance or following claim'})+'\n');
    for(let i=0;i<sequence.length;i+=9)await act(fighter,sequence.slice(i,i+9));invitation=(await observer.observe()).game_ms;
  };
  const rejoin=async goal=>{
    await act(fighter,Array.from({length:6},()=>({actor:fighter.actor,input:{keys:['-'],duration_ms:200}})));
    const before=await fighter.observe(),target=rejoinMarker((await fighter.inspect()).view,originals,goal),peer=await api({op:'observe',actor:target.actor});
    const originalPeer=v=>v.lifecycle.alive&&!v.lifecycle.death_pending&&!v.lifecycle.quarantined&&v.controller==='game_bot'&&v.session.account===target.account&&v.session.character===target.character&&v.session.world===realm&&v.character.class===target.class;
    assert(originalPeer(peer)&&peer.session.player===target.id,'Original GUI rejoin target changed before input');
    await appendFile(join(out,'earned-rejoins.jsonl'),JSON.stringify({stage:'planned',target,before:before.session,position:before.position})+'\n');
    await act(fighter,[{actor:fighter.actor,input:{keys:[],mouse:target.point,duration_ms:200}},{actor:fighter.actor,input:{keys:[],duration_ms:400}},{actor:fighter.actor,input:{keys:[],mouse:target.menu,duration_ms:200}}]);
    const after=await fighter.observe(),current=await api({op:'observe',actor:target.actor});
    assert(originalPeer(current)&&after.lifecycle.alive&&after.session.account===first.session.account&&after.session.character===first.session.character&&after.session.world===realm,'GUI rejoin changed an original identity or world');
    fighter.check('ordinary minimap GUI rejoin reaches an original living cohort ally after Nexus recovery',after.game_ms>=before.game_ms&&after.game_ms-before.game_ms<10000&&distance(before.position,after.position)>100&&distance(after.position,current.position)<=8);
    await appendFile(join(out,'earned-rejoins.jsonl'),JSON.stringify({stage:'observed',target:target.account,before:before.position,after:after.position,peer:current.position,gameMs:after.game_ms,claim:'Actual long-range same-Realm rejoin within normal moving-ally bounds, not a dispatch-only claim'})+'\n');
  };
  const walk=async(client,goal)=>{
    const world=(await client.observe()).session.world;
    for(let n=0;n<80;n++){
      if(client===fighter)await invite();const v=await client.observe();if(v.session.world!==world)return;
      if(client===fighter&&world!==-2&&(ledger?.snapshot().pending.length===0||recoveryState(v.character).retreat))return;
      if(v.character.conditions&(1<<10)){await appendFile(join(out,'earned-mobility.jsonl'),JSON.stringify({actor:client.actor,gameMs:v.game_ms,position:v.position,conditions:v.character.conditions,claim:'Observed native Confused; planned ordinary 2000ms idle, not a movement claim'})+'\n');await act(client,{keys:[],duration_ms:2000});continue;}
      const dx=goal.x-v.position.x,dy=goal.y-v.position.y,d=distance(goal,v.position);if(d<.6&&Math.floor(goal.x)===Math.floor(v.position.x)&&Math.floor(goal.y)===Math.floor(v.position.y))return;
      const keys=walkingKeys(dx,dy);
      await act(client,{keys,duration_ms:Math.min(10000,Math.max(80,Math.round(d*1000/7.734)))});
    }throw Error('Collision-legal waypoint budget exhausted');
  };
  const enter=async(client,portal)=>{await walk(client,portal.position);await act(client,{keys:[],mouse:{x:700,y:575},duration_ms:1000});assert.equal((await client.observe()).world,'Realm');};
  const nexus=(await observer.inspect()).view.objects.filter(o=>o.kind===0x712);assert.equal(nexus.length,2);const entrance=nexus[0],other=nexus[1];await walk(observer,other.position);await enter(fighter,entrance);
  const entered=await fighter.observe(),realm=entered.session.world,info=await api({op:'world_info',world:realm});let terrain=await map(fighter,true);
  const initial=terrain.entities.filter(e=>heroes.includes(e.kind)||roots.includes(e.kind)&&e.quest===e.id);ledger=earnedLedger(realm,initial.map(e=>e.id),events);await logs();assert(ledger.snapshot().census&&ledger.snapshot().credited.length===0,'Unreduced initial roster must precede combat');
  const otherScopes=[...new Set(events.filter(e=>e.message==='Realm portal updated'&&e.name===other.name&&e.open===true).map(e=>e.realm))];assert(otherScopes.length===1&&Number.isSafeInteger(otherScopes[0])&&otherScopes[0]!==realm,'Other entrance needs its distinct initial Realm binding');const independent={...other,realm:otherScopes[0]};
  const selection=mapInput(info.realm_layout,JSON.parse(await readFile('data/realm-catalog.json','utf8')));let file;
  if(selection.recipe){const recipe=JSON.parse(await readFile(join('data/realms',selection.recipe),'utf8'));assert.equal(recipe.id,info.realm_layout);file=join('data/realms',recipe.output);}else file=selection.file;
  const raw=await readFile(file);assert.equal(createHash('sha256').update(raw).digest('hex'),selection.sha256);let collision=collisionMap(terrain);
  await writeFile(join(out,'earned-source.json'),JSON.stringify({scope:'Prepared nearby cooperative raid, ordinary unreduced quest/event combat; not coast/level1/history/population acceptance. Named follow chat uses the existing test-game-bot legacy invitation path (60s), not public dialogue or LLM acceptance. Auxiliary initial positions are declared before their own enrollment. No existing actor relocation, protection, direct kill, progress injection or replacement characters.',runtime:bootstrap.runtime,source:JSON.parse(await readFile(join(native,'result.json'),'utf8')).source,initialRoots:initial,map:{id:info.realm_layout,file,sha256:createHash('sha256').update(raw).digest('hex')},caps}));
  const starts=partyPositions(collision,entered.position,profiles.length).map(position=>({position}));
  const cohort=randomUUID();for(let i=0;i<profiles.length;i++){
    const spec={guid:'earned-'+cohort+'-'+i,...auxiliary(profiles[i],i),world:realm,position:starts[i].position},created=await api({op:'spawn',spec}),v=await api({op:'observe',actor:created.actor});assert(created.connected&&created.controller==='game_bot'&&created.world===realm&&v.registered_profile===profiles[i]);assert.deepEqual(v.position,spec.position,'Initial auxiliary placement differs from its declared fixture');originals.push(originalIdentity(profiles[i],spec,v));await writeFile(join(out,'earned-originals.json'),JSON.stringify({runtime:bootstrap.runtime,originals,fronts:starts}));
    await act(fighter,{keys:[],duration_ms:1000});const known=(await fighter.inspect()).view.objects.find(p=>p.id===v.session.player);
    assert(known&&known.account===v.session.account&&known.name===profiles[i]&&known.kind===spec.class&&known.hp>0,'New original peer must be client-known before its local invitation');
    await act(fighter,{keys:[],duration_ms:1100,chat:profiles[i]+' follow me'});if(i===0)invitation=(await observer.observe()).game_ms;
    assert(events.some(e=>e.message==='autonomous follow selected'&&e.account===v.session.account&&e.target===entered.session.player&&e.world===realm),'New original peer did not select a normal follow action');
    await appendFile(join(out,'earned-enrollment-calls.jsonl'),JSON.stringify({role:profiles[i],actor:created.actor,account:v.session.account,player:v.session.player,native:entered.session.player,claim:'Original peer published and selected normal follow; not proof of sustained movement, combat or whole-clear acceptance'})+'\n');
  }
  assert(originals.length===14&&new Set(originals.map(o=>o.account)).size===14&&new Set(originals.map(o=>o.character)).size===14);
  await save('initial-party');
  const published=await save('published-party');
  fighter.check('ordinary publication exposes exact original prepared peers to the client-known view',originals.slice(2).every(o=>published.objects.some(p=>p.id===o.initial.session.player&&p.account===o.account&&p.kind===o.class&&p.name===o.role&&p.hp>0)));
  for(let iteration=0;iteration<300;iteration++){
    await logs();if(ledger.snapshot().pending.length===0)break;
    await invite();const v=await fighter.observe();assert(v.lifecycle.alive&&v.session.world===realm,'Original fighter left combat before earned closure');
    if(recoveryState(v.character).retreat){
      await save('retreat-'+iteration);await act(fighter,{keys:['Insert'],duration_ms:1000});assert.equal((await fighter.observe()).world,'Nexus');
      for(let n=0;!recoveryState((await fighter.observe()).character).recovered;n++){assert(n<30,'Ordinary Nexus regeneration did not restore the original reserve');await act(fighter,{keys:[],duration_ms:5000});}
      const portal=(await observer.inspect()).view.objects.find(o=>o.id===entrance.id);assert(portal?.usable,'Original entrance closed while the original recovered');await enter(fighter,portal);
      await rejoin(v.position);await save('recovered-'+iteration);continue;
    }
    terrain=await map(fighter,true);collision=collisionMap(terrain);const pending=new Set(ledger.snapshot().pending),family=terrain.entities.filter(e=>(pending.has(e.id)||pending.has(e.quest)||pending.has(e.parent))&&e.hp>0).sort((a,b)=>distance(a.pos,v.position)-distance(b.pos,v.position));
    if(!family.length){assert(ledger.snapshot().waiting.length>0,'Pending ledger has neither a living family nor an actual waiting event');await act(fighter,{keys:[],duration_ms:3000});continue;}
    const view=(await fighter.inspect()).view,visible=localTargets(view.objects,family,v.position),target=visible[0];
    if(!target){const points=route(collision,v.position,family[0].pos,view.objects);assert(points.length>1,'Nearby pending family is absent from the native neighborhood');await appendFile(join(out,'earned-route-plans.jsonl'),JSON.stringify({iteration,gameMs:v.game_ms,from:v.position,target:family[0],points,claim:'Planned collision/hazard-safe route only; actual movement remains in native client receipts'})+'\n');await walk(fighter,points[1]);}
    // An in-flight shot can finish this target during resume; a held aim is not a live-entity assertion.
    else{const heal=v.character.hp<v.character.stats.max_hp*.8&&v.character.mp>=85,mouse=observedAim(view,target.id,-Math.PI/4);await appendFile(join(out,'earned-aims.jsonl'),JSON.stringify({actor:fighter.actor,character:view.session.character,gameMs:view.time_ms,tick:view.tick,target:target.id,targetHP:target.hp,targetPosition:target.position,playerPosition:view.position,mouse,claim:'planned held aim at last observed position, not dispatch-time liveness or a hit receipt'})+'\n');await act(fighter,{keys:heal?[' ']:[],duration_ms:5000,mouse});}
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
  const nexusMap=await map(observer,false),fresh=replacementEntrance(handoff.replacement,nexusMap.entities,events),independentOutcome=otherEntrance(independent,nexusMap.entities,events);assert(fresh.id!==entrance.id&&fresh.id!==other.id);await walk(observer,fresh.pos);
  const portals=(await observer.inspect()).view.objects.filter(o=>o.kind===0x712),successor=portals.find(o=>o.id===fresh.id);assert(successor&&successor.usable);
  await act(observer,{keys:[],duration_ms:3000},'earned-published-successor');await act(fighter,{keys:['Insert'],duration_ms:1500});assert.equal((await fighter.observe()).world,'Nexus');await enter(fighter,successor);assert.equal((await fighter.observe()).session.world,handoff.replacement);await act(fighter,{keys:['Insert'],duration_ms:1500});
  const final=await save('final');for(const [before,after]of [[first,final],[observerFirst,await observer.observe()]]){assert(after.lifecycle.alive);assert.equal(after.session.character,before.session.character);assert.equal(after.session.account,before.session.account);assert.deepEqual(after.character.items,before.character.items);assert.equal(after.character.inventory_revision,before.character.inventory_revision);}
  await writeFile(join(out,'earned-handoff.json'),JSON.stringify({handoff,departure,publication,independentOutcome,qualification:'Actual source-conditioned Directory deadline/order and lawful other-Realm retirement if observed, not an independently measured departure interval, forced negative-pending probe, every-recipient or restart-durability proof.'}));
  fighter.check('unreduced normal census and every admitted event earned; original native receives notices and Castle; exact successor entered with durable original inventory',completed.pending.length===0&&completed.admitted.length>0&&final.lifecycle.alive&&castle.session.character===first.session.character&&successor.id===fresh.id);
  await writeFile(join(out,'earned-subchecks.json'),JSON.stringify({functional:'PASS',earned:completed,originalNativeSurvival:'PASS',auxiliaryOutcomes:'REQUIRES_OWN_TERMINAL_COLD_AUDIT',nativeCameraShake:'NOT_CLAIMED',historical:'NOT_CLAIMED',cohort}));
}
