import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFile} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {promisify} from 'node:util';
import {journaledReward} from './reward-journal.mjs';
const exec=promisify(execFile),out=process.env.QA_RESULTS_DIR;
// event_rng = StdRng(seed^world^0x7265616c6d): this seed's first qualifying defeat wins the .25 roll and chooses 0xd59.
export const realmSeed=6;
// Piercing fire avoids chasing orbiting followers through untouched native scenery.
const spec={class:0x307,level:20,seed:820,world:1,equipment:[0xc02,-1,0xaf9,0xac7],backpack:Array(8).fill(-1),stats:[700,252,75,25,50,50,40,50]};
let scout,roots,earned,origin,token,spawn;
const protectedUntil=new Map();
async function api(command){const r=await fetch(origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command})});const v=await r.json();assert(r.ok&&v.status!=='failed',JSON.stringify(v));return ['observe','inspect','world_info'].includes(command.op)?v:v.result;}
// Protection cannot be renewed while active, and loaded acts overrun: once in the run, idle out a nearly spent window before renewing.
let idler;
// The idle-to-expiry overruns into an unprotected gap where Cube Overseers killed the Archer (VPS 95264c2e): enter it at full HP.
// (Only one prepared stat modifier may be active, and the fight already holds the attack fixture.)
async function protect(actor){let now=(await api({op:'observe',actor})).game_ms;const until=protectedUntil.get(actor)??0;
  if(idler&&now<until&&until-now<6000){const v=await api({op:'observe',actor});await api({op:'cheat',actor,change:{kind:'vitals',hp:v.character.stats.max_hp,mp:v.character.mp}});}
  while(idler&&now<until&&until-now<6000){await idler.act({keys:[],duration_ms:Math.max(50,until-now)});now=(await api({op:'observe',actor})).game_ms;}if(now>=until){await api({op:'cheat',actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});protectedUntil.set(actor,now+60000);}}
const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
async function fields(){return (await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);}
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];spawn={x,y};
  scout=(await api({op:'spawn',spec:{...spec,guid:'cube-admission-'+randomUUID(),position:{x,y},controller:'direct',profile:null}})).result.actor;
  const state=await api({op:'inspect',actor:scout}),report=JSON.parse(await readFile(join(out,'native/result.json'),'utf8'));
  origin=report.origin;token=(await readFile(join(out,'native/state/control.token'),'utf8')).trim();
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(response.ok);
  const kinds=[0x678,0x668,0x67d,0x91b];
  const map=await response.json();
  const blocked=new Set(map.blocked.map(p=>p.join(',')));
  const clear=map.entities.filter(e=>e.hp>0);
  const safe=world.spawn_points.flatMap(([x,y])=>Array.from({length:25},(_,i)=>({x:x+i%5-2,y:y+Math.floor(i/5)-2})))
    .find(p=>!blocked.has([Math.floor(p.x),Math.floor(p.y)].join(','))&&clear.every(e=>Math.hypot(e.pos.x-p.x,e.pos.y-p.y)>14));
  assert(safe,'Expected a coast tile outside the ordinary /pause threat radius');spawn=safe;
  roots=map.entities.filter(e=>kinds.includes(e.kind)).map(e=>({entity:e.id,kind:e.kind,x:e.pos.x,y:e.pos.y}));
  roots.sort((a,b)=>kinds.indexOf(a.kind)-kinds.indexOf(b.kind));assert(roots.length>0);
  await writeFile(join(out,'placement.json'),JSON.stringify({world,roots,scout}));
  return {...spec,position:()=>{assert(earned,'Admission must precede native placement');return {x:earned.x,y:earned.y};}};
}
export async function beforeNavigate(){
  if(earned)return;
  const config=join(out,'direct.json');await writeFile(config,JSON.stringify({origin,state:join(out,'native/state')}));
  let serial=0;
  for(const root of roots){
    if(earned)break;
    await api({op:'cheat',actor:scout,change:{kind:'teleport',position:{x:root.x,y:root.y}}});
    for(let n=0;n<30;n++){
      await protect(scout);
      const state=await api({op:'inspect',actor:scout}),target=state.diagnostic.entities.find(e=>(e.id===root.entity||e.quest===root.entity)&&[root.kind,0x91c].includes(e.kind));
      if(!target)break;
      await api({op:'cheat',actor:scout,change:{kind:'teleport',position:target.pos}});
      const request=join(out,'command-'+serial+++'.json');await writeFile(request,JSON.stringify({id:randomUUID(),command:{op:'act',actor:scout,action:{command:'shoot',angle:0},duration_ms:2000}}));
      // The runner must keep draining backend logs while its controller awaits the server.
      await exec(process.execPath,[resolve('scripts/game.mjs'),config,request]);
    }
    earned=(await fields()).find(f=>f.message==='quest defeat summoned a Realm event'&&f.kind===0xd59);
  }
  assert(earned,'No Cube God earned within the finite qualifying-quest sample');
  await writeFile(join(out,'admission.json'),JSON.stringify(earned));
  await api({op:'cheat',actor:scout,change:{kind:'teleport',position:spawn}});
  await api({op:'cheat',actor:scout,change:{kind:'pause',paused:true}});
}
export async function run(game){
  idler=game;
  game.check('qualifying combat admits an ordinarily selected Cube God',roots.some(r=>r.entity===earned.source));
  const spectator={actor:scout,observe:()=>api({op:'observe',actor:scout}),inspect:()=>api({op:'inspect',actor:scout})},root=earned.entity;
  await protect(game.actor);
  const snapshots=[];
  async function save(label){const s=await game.inspect();snapshots.push({label,state:s});await writeFile(join(out,'encounter.json'),JSON.stringify(snapshots));return s;}
  // XML ownership: Cube God MakeMinions Overseers, each Overseer MakeMinions Defenders/Blasters (actors.xml:8240,8267).
  const tree=list=>{const ids=new Set([root]);for(let grew=true;grew;){grew=false;for(const e of list)if(!ids.has(e.id)&&ids.has(e.parent)){ids.add(e.id);grew=true;}}return list.filter(e=>ids.has(e.id));};
  const family=s=>tree(s.diagnostic.entities).filter(e=>e.kind>=0xd5a&&e.kind<=0xd5c);
  const first=await save('admitted');
  game.check('admitted root retains selected full health and stable ownership',entity(first,root)?.hp===10000&&entity(first,root)?.quest===root);
  const trace=[];
  for(let n=0;n<12;n++){
    await protect(game.actor);await protect(spectator.actor);
    await game.act({keys:[],duration_ms:500});
    const s=await game.inspect();trace.push({time:s.view.time_ms,entities:tree(s.diagnostic.entities),projectiles:s.view.projectiles});
  }
  await writeFile(join(out,'cycle.json'),JSON.stringify(trace));
  let state=await save('active');
  game.check('all three dependent species activate through ordinary reproduction',[0xd5a,0xd5b,0xd5c].every(k=>family(state).some(e=>e.kind===k&&e.activated!==null)));
  game.check('Cube God wanders rather than remaining at its birth point',trace.some(t=>{const e=t.entities.find(e=>e.id===root);return e&&Math.hypot(e.pos.x-earned.x,e.pos.y-earned.y)>.2;}));
  game.check('owned followers move around their encounter', [0xd5a,0xd5b,0xd5c].every(k=>trace.some((t,n)=>t.entities.some(e=>e.kind===k&&trace.slice(0,n).some(p=>p.entities.some(old=>old.id===e.id&&Math.hypot(e.pos.x-old.pos.x,e.pos.y-old.pos.y)>.2))))));
  game.check('native session sees root and every dependent attack',[0xd59,0xd5a,0xd5b,0xd5c].every(k=>trace.some(t=>t.projectiles.some(p=>t.entities.some(e=>e.kind===k&&e.id===p.owner)))));
  // The existing temporary attack fixture keeps minion combat within the protection window.
  await api({op:'cheat',actor:game.actor,change:{kind:'stat_modifier',index:2,amount:100,duration_ms:60000}});
  // Reproduce is a nearby density cap, not a per-parent lifetime budget: https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/wServer/logic/behaviors/Reproduce.cs
  const distance=e=>Math.hypot(e.pos.x-state.view.position.x,e.pos.y-state.view.position.y);
  async function approach(actor,destination,range=1.5){
    for(let n=0;n<80;n++){
      await protect(actor.actor);await protect(spectator.actor);
      const point=typeof destination==='number'?entity(await actor.inspect(),destination)?.pos:destination;
      if(!point){assert((await fields()).some(f=>f.message==='enemy defeated'&&f.entity===destination),'Walking target vanished without a logged defeat');return false;}
      const view=await actor.observe(),dx=point.x-view.position.x,dy=point.y-view.position.y;if(Math.hypot(dx,dy)<range)return true;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      const distance=keys.length===2?Math.min(Math.abs(x),Math.abs(y))*Math.SQRT2:Math.max(Math.abs(x),Math.abs(y));
      await actor.act({keys,duration_ms:Math.max(50,Math.min(750,Math.round(distance/(4+5.6*view.character.stats.speed/75)*1000)))});
    }throw Error('Native walk did not reach target');
  }
  async function fire(actor,id,ms=250){await protect(actor.actor);const s=await actor.inspect(),e=entity(s,id);assert(e);if(Math.hypot(e.pos.x-s.view.position.x,e.pos.y-s.view.position.y)>6&&!await approach(actor,id,5.5))return;await actor.act({keys:[],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});}
  await game.act({keys:[],duration_ms:750},{name:'cube-active',when:()=>true});
  const census=await fetch(origin+'/dev/map?player='+(await game.inspect()).diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(census.ok);
  const owned=tree((await census.json()).entities),old=new Set(owned.map(e=>e.id));await writeFile(join(out,'census.json'),JSON.stringify(owned));
  game.check('owned Cube census respects XML caps of ten Overseers with five minions each',owned.filter(e=>e.parent===root).length<=10&&owned.filter(e=>e.parent!==root&&e.id!==root).every(e=>owned.some(o=>o.id===e.parent&&o.kind===0xd5a))&&owned.filter(e=>e.kind===0xd5a).every(o=>owned.filter(e=>e.parent===o.id).length<=5)&&owned.some(e=>e.id===root)&&owned.some(e=>e.kind===0xd5a));
  let killed;
  for(let n=0;n<30&&!killed;n++){
    state=await game.inspect();const target=family(state).filter(e=>e.kind===0xd5a&&old.has(e.id)&&state.view.objects.some(o=>o.id===e.id)).sort((a,b)=>distance(a)-distance(b))[0];assert(target);
    await protect(game.actor);await game.act({keys:[],duration_ms:250,target:{entity:target.id,camera_angle:-Math.PI/4,fire:true}});killed=(await fields()).find(f=>f.message==='enemy defeated'&&f.kind===0xd5a&&old.has(f.entity));
  }
  game.check('native bow fire defeats an Overseer',Boolean(killed));
  for(let n=0;n<16&&!family(await game.inspect()).some(e=>!old.has(e.id));n++){await protect(game.actor);await game.act({keys:[],duration_ms:1000});}
  state=await save('replenished');game.check('owned Cube population replenishes after native combat',family(state).some(e=>!old.has(e.id)));
  assert(entity(state,root));
  const partner=await game.join({...spec,seed:821,position:state.view.position});await protect(partner.actor);
  const characters=(await Promise.all([game.inspect(),partner.inspect()])).map(s=>s.view.session.character);
  for(const [i,actor] of [game,partner].entries()){for(let n=0;n<8&&(entity(await game.inspect(),root).damage[characters[i]]??0)<600;n++)await fire(actor,root,150);}
  state=await save('contributors');game.check('two native clients contribute above the 500-damage threshold',characters.every(id=>entity(state,root).damage[id]>500));
  const xp=await Promise.all([game.observe(),partner.observe(),spectator.observe()]);
  // The Doom Bow fires ~2 shots/s (RateOfFire .33), so 250ms volleys often release none; 70 of them left the God at 1794 HP (local 1dbd8cc3).
  // The idle partner also needs renewals across the longer volley loop (its lapsed protection let the Cube God kill it, local bd455c90).
  // A God wedged among Trees (OccupySquare cover; the Doom Bow lacks PassesCover) stops every arrow: local 7920d20d and 259f77d4 held it at
  // 1794 and 3542 HP for 40 volleys. When HP stalls, stand on the God so no cover lies between.
  for(let n=0,last,stalled=0;n<70&&entity(await game.inspect(),root);n++){
    await protect(partner.actor);const hp=entity(await game.inspect(),root).hp;stalled=hp===last?stalled+1:0;last=hp;
    if(stalled>=3){await approach(game,root,.6);stalled=0;}
    await fire(game,root,1000);
  }
  state=await save('defeated');game.check('native bow combat defeats the admitted Cube God',!entity(state,root));
  const after=await Promise.all([game.inspect(),partner.inspect(),spectator.inspect()]);
  game.check('both native contributors earn XP while distant spectator does not',after[0].view.character.experience>xp[0].character.experience&&after[1].view.character.experience>xp[1].character.experience&&after[2].view.character.experience===xp[2].character.experience);
  // Rewards publish from the journaled terminal allocation; its Potion row has min 1 per encounter, so an eligible contributor always has a bag.
  const journal={origin,state:join(out,'native/state')},rewards=await Promise.all(after.slice(0,2).map(s=>journaledReward(journal,root,s.view.session.character)));
  game.check('journaled Cube God allocations follow its XML private rows for both contributors',rewards.every(r=>r.valid));
  const index=rewards.findIndex(r=>r.items.length);assert(index>=0,'The min-1 Potion row must allocate to an eligible contributor');
  const owner=[game,partner][index],ownerState=await owner.inspect(),bag=ownerState.diagnostic.entities.find(e=>e.extra_stats.some(([k,a])=>k===54&&a===ownerState.view.session.account)&&JSON.stringify((e.items??[]).filter(i=>i!==-1).sort())===JSON.stringify([...rewards[index].items].sort()));
  assert(bag,'Journaled Cube allocation must publish its private bag');
  await approach(owner,bag.pos,.4);await owner.act({keys:[],duration_ms:750},{name:'cube-reward',when:()=>true});
  const inventory=(await owner.observe()).character,item=bag.items[0];assert(item!==-1);
  await owner.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  const picked=await owner.inspect();await writeFile(join(out,'pickup.json'),JSON.stringify({bag,inventory,picked}));
  game.check('native bag drag commits exactly one eligible reward',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===inventory.inventory_revision+1&&(!entity(picked,bag.id)||entity(picked,bag.id).items[0]===-1));
  await owner.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:678,y:428}}});game.check('empty bag drag cannot duplicate reward',(await owner.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  await owner.act({keys:['Insert'],duration_ms:250});const retained=await owner.observe();game.check('Nexus reconnect preserves living identity and reward',retained.world==='Nexus'&&retained.lifecycle.alive&&retained.session.character===picked.view.session.character&&retained.character.items[4]===item&&retained.character.inventory_revision===picked.view.character.inventory_revision);
  await writeFile(join(out,'retained.json'),JSON.stringify(retained));
}
