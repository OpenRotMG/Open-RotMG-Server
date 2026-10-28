import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
import {randomUUID} from 'node:crypto';
const out=process.env.QA_RESULTS_DIR;
const spec={class:0x31d,level:20,seed:820,world:1,equipment:[0xb0b,0xb29,0xa7b,-1],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,50]};
let scout,roots,earned,origin,token,spawn;
const protectedUntil=new Map();
async function api(command){const r=await fetch(origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command})});const v=await r.json();assert(r.ok&&v.status!=='failed',JSON.stringify(v));return ['observe','inspect','world_info'].includes(command.op)?v:v.result;}
async function protect(actor){const now=(await api({op:'observe',actor})).game_ms;if(now>=(protectedUntil.get(actor)??0)){await api({op:'cheat',actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});protectedUntil.set(actor,now+60000);}}
const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
async function fields(){return (await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);}
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];spawn={x,y};
  scout=(await api({op:'spawn',spec:{...spec,guid:'cube-admission-'+randomUUID(),position:{x,y},controller:'direct',profile:null}})).result.actor;
  const state=await api({op:'inspect',actor:scout}),report=JSON.parse(await readFile(join(out,'native/result.json'),'utf8'));
  origin=report.origin;token=(await readFile(join(out,'native/state/control.token'),'utf8')).trim();
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(response.ok);
  const kinds=[0x678,0x668,0x67d,0x91b];
  roots=(await response.json()).entities.filter(e=>kinds.includes(e.kind)).map(e=>({entity:e.id,kind:e.kind,x:e.pos.x,y:e.pos.y}));
  roots.sort((a,b)=>kinds.indexOf(a.kind)-kinds.indexOf(b.kind));assert(roots.length>0);
  await writeFile(join(out,'placement.json'),JSON.stringify({world,roots,scout}));
  return {...spec,position:'spawn'};
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
      const request=join(out,'command-'+serial+++'.json');await writeFile(request,JSON.stringify({id:randomUUID(),command:{op:'act',actor:scout,action:{command:'shoot',angle:0},duration_ms:1000}}));
      execFileSync(process.execPath,[resolve('scripts/game.mjs'),config,request],{stdio:['ignore','pipe','pipe']});
    }
    earned=(await fields()).find(f=>f.message==='quest defeat summoned a Realm event'&&f.kind===0xd59);
  }
  assert(earned,'No Cube God earned within the finite qualifying-quest sample');
  await writeFile(join(out,'admission.json'),JSON.stringify(earned));
  await api({op:'cheat',actor:scout,change:{kind:'teleport',position:spawn}});
  await api({op:'cheat',actor:scout,change:{kind:'pause',paused:true}});
}
export async function run(spectator){
  spectator.check('qualifying combat admits an ordinarily selected Cube God',roots.some(r=>r.entity===earned.source));
  await protect(spectator.actor);
  const game=await spectator.join({...spec,position:{x:earned.x,y:earned.y}}),root=earned.entity;
  await protect(game.actor);
  const snapshots=[];
  async function save(label){const s=await game.inspect();snapshots.push({label,state:s});await writeFile(join(out,'encounter.json'),JSON.stringify(snapshots));return s;}
  const family=s=>s.diagnostic.entities.filter(e=>e.quest===root&&e.kind>=0xd5a&&e.kind<=0xd5c);
  const first=await save('admitted');
  game.check('admitted root retains selected full health and stable ownership',entity(first,root)?.hp===10000&&entity(first,root)?.quest===root);
  const trace=[];
  for(let n=0;n<24;n++){
    await protect(game.actor);await protect(spectator.actor);
    await game.act({keys:[],duration_ms:500});
    const s=await game.inspect();trace.push({time:s.view.time_ms,entities:s.diagnostic.entities.filter(e=>e.quest===root),projectiles:s.view.projectiles});
  }
  await writeFile(join(out,'cycle.json'),JSON.stringify(trace));
  let state=await save('active');
  game.check('all three dependent species activate through ordinary reproduction',[0xd5a,0xd5b,0xd5c].every(k=>family(state).some(e=>e.kind===k&&e.activated!==null)));
  game.check('Cube God wanders rather than remaining at its birth point',trace.some(t=>{const e=t.entities.find(e=>e.id===root);return e&&Math.hypot(e.pos.x-earned.x,e.pos.y-earned.y)>.2;}));
  game.check('owned followers move around their encounter', [0xd5a,0xd5b,0xd5c].every(k=>trace.some((t,n)=>t.entities.some(e=>e.kind===k&&trace.slice(0,n).some(p=>p.entities.some(old=>old.id===e.id&&Math.hypot(e.pos.x-old.pos.x,e.pos.y-old.pos.y)>.2))))));
  game.check('native session sees root and every dependent attack',[0xd59,0xd5a,0xd5b,0xd5c].every(k=>trace.some(t=>t.projectiles.some(p=>t.entities.some(e=>e.kind===k&&e.id===p.owner)))));
  // Reproduce is a nearby density cap, not a per-parent lifetime budget: https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/wServer/logic/behaviors/Reproduce.cs
  const distance=e=>Math.hypot(e.pos.x-state.view.position.x,e.pos.y-state.view.position.y);
  const victim=family(state).filter(e=>e.kind===0xd5a&&state.view.objects.some(o=>o.id===e.id)).sort((a,b)=>distance(a)-distance(b))[0];assert(victim);
  async function approach(actor,destination,range=1.5){
    for(let n=0;n<80;n++){
      await protect(actor.actor);await protect(spectator.actor);
      const point=typeof destination==='number'?entity(await actor.inspect(),destination)?.pos:destination;assert(point,'Walking target must still exist');
      const view=await actor.observe(),dx=point.x-view.position.x,dy=point.y-view.position.y;if(Math.hypot(dx,dy)<range)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      const distance=keys.length===2?Math.min(Math.abs(x),Math.abs(y))*Math.SQRT2:Math.max(Math.abs(x),Math.abs(y));
      await actor.act({keys,duration_ms:Math.max(50,Math.min(750,Math.round(distance/(4+5.6*view.character.stats.speed/75)*1000)))});
    }throw Error('Native walk did not reach target');
  }
  async function fire(actor,id,ms=250){await protect(actor.actor);const s=await actor.inspect(),e=entity(s,id);assert(e);if(Math.hypot(e.pos.x-s.view.position.x,e.pos.y-s.view.position.y)>2)await approach(actor,id);await actor.act({keys:[],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});}
  await game.act({keys:[],duration_ms:750},{name:'cube-active',when:()=>true});
  const census=await fetch(origin+'/dev/map?player='+(await game.inspect()).diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(census.ok);
  const owned=(await census.json()).entities.filter(e=>e.quest===root),old=new Set(owned.filter(e=>e.kind===0xd5a).map(e=>e.id));await writeFile(join(out,'census.json'),JSON.stringify(owned));
  game.check('global owned Cube census respects the existing 32-actor budget',owned.length<=32&&owned.some(e=>e.id===root)&&old.has(victim.id));
  const mana=(await game.observe()).character.mp;
  await game.act({keys:[' '],duration_ms:150});
  game.check('ordinary native helm spends mana and grants Speedy for pursuit',(await game.observe()).character.mp<mana&&((await game.observe()).character.conditions&(1<<14))!==0);
  for(let n=0;n<30&&entity(await game.inspect(),victim.id);n++)await fire(game,victim.id);
  game.check('native sword fire defeats an Overseer',!entity(await game.inspect(),victim.id));
  for(let n=0;n<15&&!family(await game.inspect()).some(e=>e.kind===0xd5a&&!old.has(e.id));n++)await game.act({keys:[],duration_ms:1000});
  state=await save('replenished');const births=await fields(),removed=births.findIndex(f=>f.message==='enemy defeated'&&f.entity===victim.id);
  game.check('living Cube replenishes an owned Overseer',family(state).some(e=>e.kind===0xd5a&&!old.has(e.id))&&removed>=0&&births.slice(removed+1).some(f=>f.message==='bounded named quest followers spawned'&&f.quest===root&&f.kind===0xd5a));
  const boss=entity(state,root);assert(boss);
  const partner=await game.join({...spec,seed:821,position:boss.pos});await protect(partner.actor);
  await game.act({keys:[' '],duration_ms:150});
  for(const actor of [game,partner]){for(let n=0;n<8&&(entity(await game.inspect(),root).damage[actor.actor]??0)<600;n++)await fire(actor,root,150);}
  state=await save('contributors');game.check('two native clients contribute above the 500-damage threshold',[game,partner].every(a=>entity(state,root).damage[a.actor]>500));
  const xp=await Promise.all([game.observe(),partner.observe(),spectator.observe()]);
  for(let n=0;n<70&&entity(await game.inspect(),root);n++)await fire(game,root);
  state=await save('defeated');game.check('native sword combat defeats the admitted Cube God',!entity(state,root));
  const after=await Promise.all([game.inspect(),partner.inspect(),spectator.inspect()]);
  game.check('both native contributors earn XP while distant spectator does not',after[0].view.character.experience>xp[0].character.experience&&after[1].view.character.experience>xp[1].character.experience&&after[2].view.character.experience===xp[2].character.experience);
  const log=await fields(),death=log.findIndex(f=>f.message==='enemy defeated'&&f.entity===root);assert(death>=0&&log.filter(f=>f.message==='enemy defeated'&&f.entity===root).length===1,'One logged Cube defeat required');
  const previous=log.slice(0,death).findLastIndex(f=>f.message==='enemy defeated'),cubeBags=new Set(log.slice(previous+1,death).filter(f=>f.message==='loot bag allocated').map(f=>f.entity));
  const bags=after.slice(0,2).flatMap(s=>s.diagnostic.entities).filter(e=>cubeBags.has(e.id)&&e.extra_stats.some(([k,a])=>k===54&&a>=0)&&e.items?.some(i=>i!==-1));assert(bags.length,'Observed Cube roll must provide loot; no probability inferred');
  const owner=[game,partner].find((a,i)=>bags.some(b=>b.extra_stats.some(([k,v])=>k===54&&v===after[i].view.session.account)));assert(owner);
  const ownerState=await owner.inspect(),bag=bags.find(b=>b.extra_stats.some(([k,v])=>k===54&&v===ownerState.view.session.account));
  await approach(owner,bag.pos,.4);await owner.act({keys:[],duration_ms:750},{name:'cube-reward',when:()=>true});
  const inventory=(await owner.observe()).character,item=bag.items[0];assert(item!==-1);
  await owner.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  const picked=await owner.inspect();await writeFile(join(out,'pickup.json'),JSON.stringify({bag,inventory,picked}));
  game.check('native bag drag commits exactly one eligible reward',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===inventory.inventory_revision+1&&entity(picked,bag.id)?.items[0]===-1);
  await owner.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:678,y:428}}});game.check('empty bag drag cannot duplicate reward',(await owner.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  await owner.act({keys:['Insert'],duration_ms:250});const retained=await owner.observe();game.check('Nexus reconnect preserves living identity and reward',retained.world==='Nexus'&&retained.lifecycle.alive&&retained.session.character===picked.view.session.character&&retained.character.items[4]===item&&retained.character.inventory_revision===picked.view.character.inventory_revision);
  await writeFile(join(out,'retained.json'),JSON.stringify(retained));
}
