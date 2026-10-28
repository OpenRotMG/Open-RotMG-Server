import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const spec={class:0x30e,level:3,seed:9009,world:1,equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,20,0,30,20,0,0]};
let crab;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'crab-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor}),root=join(process.env.QA_RESULTS_DIR,'native');
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  crab=map.entities.find(e=>e.kind===0x6c4&&Array.from({length:7},(_,i)=>i-4).every(dx=>Array.from({length:5},(_,i)=>i-2).every(dy=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))));
  assert(crab,'No naturally allocated Giant Crab with a clear native approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-placement.json'),JSON.stringify({crab,crabs:map.entities.filter(e=>e.kind===0x6c4)}));
  return {...spec,position:{x:crab.pos.x-3,y:crab.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const entity=s=>s.diagnostic.entities.find(e=>e.id===crab.id);
  const begin=await save('arrival');
  game.check('natural Realm Crab has selected 300 HP and size 100',entity(begin)?.hp===300&&entity(begin).size===100);
  await game.act({keys:[],duration_ms:500},{name:'crab-before',when:()=>true});
  const samples=[];
  for(let n=0;n<70;n++){await game.act({keys:[],duration_ms:100});samples.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'movement-and-attacks.json'),JSON.stringify(samples));
  game.check('level-three client receives a Giant Crab quest',samples.some(s=>s.view.quest===crab.id));
  game.check('Crab activates and scuttles toward the native player',samples.some(s=>{const e=entity(s);return e.activated!==null&&Math.hypot(e.pos.x-crab.pos.x,e.pos.y-crab.pos.y)>.2&&Math.hypot(e.pos.x-s.view.position.x,e.pos.y-s.view.position.y)<3;}));
  // Current timing is a reconstruction, not Kithio's 400/200ms scuttle and 4700/3200ms attack phases: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Lowland.cs#L460-L518
  const shots=samples.flatMap(s=>s.view.projectiles).filter(p=>p.owner===crab.id);
  for(const [speed,lifetime] of [[20,200],[40,400],[60,600],[80,800],[70,1800]])game.check('native client receives Crab projectile '+speed+'/'+lifetime,shots.some(p=>p.speed===speed&&p.lifetime_ms===lifetime));
  game.check('unprotected native character takes encounter damage',samples.some(s=>s.view.character.hp<begin.view.character.hp));
  await game.act({keys:[],duration_ms:600},{name:'crab-attacks',when:()=>true});
  const xp=(await game.observe()).character.experience;
  for(let n=0;n<100&&entity(await game.inspect());n++)await game.act({keys:[],duration_ms:100,target:{entity:crab.id,camera_angle:-Math.PI/4,fire:true}});
  await game.act({keys:[],duration_ms:500},{name:'crab-defeated',when:()=>true});
  const dead=await save('defeated');
  game.check('native staff defeats Crab and awards at least its 44 XP',!entity(dead)&&dead.view.character.experience>=xp+44);
  await game.act({keys:[],duration_ms:8000});
  const end=await save('finished');
  game.check('Crab defeat cannot repeat its XP award',end.view.character.experience===dead.view.character.experience);
  game.check('same unprotected prepared character survives the encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
}
