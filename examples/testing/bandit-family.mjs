import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:2,seed:816,world:1,equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,25,0,30,20,0,0]};
let leader;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'bandit-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const root=join(process.env.QA_RESULTS_DIR,'native');
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  leader=map.entities.find(e=>e.kind===0x953&&Array.from({length:7},(_,i)=>!blocked.has([Math.floor(e.pos.x)-i,Math.floor(e.pos.y)].join(','))).every(Boolean));
  assert(leader,'No natural Bandit Leader with a clear approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-bandits.json'),JSON.stringify(map.entities.filter(e=>e.camp===leader.camp)));
  return {...spec,position:{x:leader.pos.x-3,y:leader.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===leader.camp&&e.kind===0x954);
  const begin=await save('arrival');
  game.check('natural ShoreSand quest has selected HP and three initial Bandits',entity(begin,leader.id).hp===280&&family(begin).length===3&&family(begin).every(e=>e.hp===50));
  await game.act({keys:[],duration_ms:2000,target:{entity:leader.id,camera_angle:-Math.PI/4,fire:false}});
  const exposed=await save('exposed');
  game.check('unprotected native Wizard takes encounter damage',exposed.view.character.hp<begin.view.character.hp);
  const path=join(process.env.QA_RESULTS_DIR,'protection.json');
  await writeFile(path,JSON.stringify({id:'bandit-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);
  const samples=[];
  for(let n=0;n<24;n++){await game.act({keys:[],duration_ms:500},n===12?{name:'bandit-family',when:()=>true}:undefined);samples.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'attacks.json'),JSON.stringify(samples));
  game.check('Leader activates and chases',samples.some(s=>{const e=entity(s,leader.id);return e.activated!==null&&Math.hypot(e.pos.x-leader.pos.x,e.pos.y-leader.pos.y)>.2;}));
  game.check('living Leader grows its reconstructed family to five',family(samples.at(-1)).length===5);
  const shots=samples.flatMap(s=>s.view.projectiles),ids=new Set(samples.flatMap(family).map(e=>e.id));
  game.check('Leader uses selected 800ms Blade',shots.some(p=>p.owner===leader.id&&p.speed===60&&p.lifetime_ms===800));
  for(const lifetime of [600,1000])game.check('Bandits use selected '+lifetime+'ms Blade',shots.some(p=>ids.has(p.owner)&&p.speed===60&&p.lifetime_ms===lifetime));
  game.check('natural followers move',samples.some(s=>family(s).some(e=>family(begin).some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.1))));
  async function defeat(id){
    const xp=(await game.observe()).character.experience;
    for(let n=0;n<60&&entity(await game.inspect(),id);n++)await game.act({keys:[],duration_ms:250,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
    const after=await game.inspect();game.check(id+': native shooting defeats target and awards XP',!entity(after,id)&&after.view.character.experience>xp);
  }
  await defeat(family(samples.at(-1))[0].id);
  // The primary comparator disagrees with this reconstruction; do not turn a current-behavior receipt into historical acceptance: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Shore.cs#L112-L162
  for(let n=0;n<80&&entity(await game.inspect(),leader.id)?.hp>80;n++)await game.act({keys:[],duration_ms:100,target:{entity:leader.id,camera_angle:-Math.PI/4,fire:true}});
  const hurt=await save('low-health');assert(entity(hurt,leader.id),'Leader died before retreat observation');
  game.check('ordinary native shooting reaches the reconstructed retreat threshold',entity(hurt,leader.id).hp<=80);
  await game.act({keys:[],duration_ms:2000},{name:'bandit-retreat',when:()=>true});
  const retreat=await save('retreat'),distance=s=>Math.hypot(entity(s,leader.id).pos.x-s.view.position.x,entity(s,leader.id).pos.y-s.view.position.y);
  game.check('injured Leader retreats from native player',distance(retreat)>distance(hurt)+.2);
  await defeat(leader.id);
  const dead=await save('leader-defeated');
  await game.act({keys:[],duration_ms:6000});await game.act({keys:[],duration_ms:6000});
  const orphans=await save('orphans');
  game.check('dead Leader cannot grow or replace the family',!entity(orphans,leader.id)&&family(orphans).every(e=>family(dead).some(p=>p.id===e.id)));
  game.check('Leader XP is not awarded twice',orphans.view.character.experience===dead.view.character.experience);
  await game.act({keys:[],duration_ms:600},{name:'bandit-defeated',when:()=>true});
  const end=await save('finished');game.check('same prepared living character completes encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
}
