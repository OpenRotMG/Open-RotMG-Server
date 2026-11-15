import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:802,world:1,equipment:[0xa9f,0xa2e,0xb05,0xac7],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
const root=process.argv[process.argv.indexOf('--run-dir')+1];
let queen;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'wasp-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  queen=map.entities.find(e=>e.kind===0x947&&[-2,-1,0,1,2].every(dx=>[-2,-1,0,1,2].every(dy=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))));assert(queen,'No clear naturally populated Wasp Queen');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-queens.json'),JSON.stringify(map.entities.filter(e=>e.kind===0x947)));
  return {...spec,position:{x:queen.pos.x-2,y:queen.pos.y}};
}
export async function run(game){
  let serial=0;
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  async function protect(){
    const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'wasp-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path]);
  }
  const members=s=>s.diagnostic.entities.filter(e=>e.camp===queen.camp&&[0x948,0x949].includes(e.kind));
  const begin=await save('arrival'),hp=begin.view.character.hp;
  game.check('ordinary Realm Queen is visible with selected HP',begin.diagnostic.entities.some(e=>e.id===queen.id&&e.kind===0x947&&e.hp===850));
  // The dated Warrior subsection must not become Queen loot: https://web.archive.org/web/20120530043951id_/http://forums.wildshadow.com/wiki/realm-mad-god/wasp-queen
  game.check('ordinary initial roles obey reconstructed bounds',[ [0x948,2,5],[0x949,1,2] ].every(([k,min,max])=>{const n=members(begin).filter(e=>e.kind===k).length;return n>=min&&n<=max;}));
  await game.act({keys:[],duration_ms:1500},{name:'wasp-arrival',when:()=>true});
  const exposed=await save('exposed');
  game.check('ordinary enemy projectiles cause native damage',exposed.view.character.hp<hp);
  await protect();
  const samples=[];
  for(let n=0;n<6;n++){await game.act({keys:[],duration_ms:700});const s=await game.inspect();samples.push(s);}
  await writeFile(join(process.env.QA_RESULTS_DIR,'movement-and-attacks.json'),JSON.stringify(samples));
  game.check('Queen activates and wanders',samples.some(s=>s.diagnostic.entities.some(e=>e.id===queen.id&&e.activated!==null&&Math.hypot(e.pos.x-queen.pos.x,e.pos.y-queen.pos.y)>.1)));
  for(const kind of [0x947,0x948,0x949]){
    const ids=new Set(begin.diagnostic.entities.filter(e=>e.camp===queen.camp&&e.kind===kind).map(e=>e.id));
    game.check(kind+': observed selected projectile speed and lifetime',samples.some(s=>s.view.projectiles.some(p=>ids.has(p.owner)&&p.speed===75&&p.lifetime_ms===1200)));
    if(kind!==0x947)game.check(kind+': owned followers move around their Queen',samples.some(s=>members(s).some(e=>e.kind===kind&&begin.diagnostic.entities.some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.2))));
  }
  async function defeat(id){
    const before=await game.observe();
    for(let n=0;n<20&&(await game.inspect()).diagnostic.entities.some(e=>e.id===id);n++)await game.act({keys:[],duration_ms:250,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
    await game.act({keys:[],duration_ms:300});
    const s=await game.inspect();
    game.check(id+': native staff defeats target and awards XP',!s.diagnostic.entities.some(e=>e.id===id)&&s.view.character.experience>before.character.experience);
  }
  for(const kind of [0x948,0x949]){
    const before=await save('before-defeat-'+kind),victim=members(before).find(e=>e.kind===kind);assert(victim);
    const ids=new Set(members(before).map(e=>e.id));await defeat(victim.id);
    for(let n=0;n<12&&!members(await game.inspect()).some(e=>e.kind===kind&&!ids.has(e.id));n++)await game.act({keys:[],duration_ms:1000});
    const after=await save('replenished-'+kind);
    game.check(kind+': living Queen replenishes the defeated role',members(after).some(e=>e.kind===kind&&!ids.has(e.id)));
    game.check(kind+': current family stays within five Workers and two Warriors',[ [0x948,5],[0x949,2] ].every(([k,max])=>members(after).filter(e=>e.kind===k).length<=max));
  }
  await game.act({keys:[],duration_ms:500},{name:'wasp-replenished',when:()=>true});
  await defeat(queen.id);
  const dead=await save('queen-defeated'),remaining=members(dead);
  game.check('Queen defeat produces no successor in its camp',dead.diagnostic.entities.filter(e=>e.camp===queen.camp).every(e=>e.kind===0x948||e.kind===0x949));
  await game.act({keys:[],duration_ms:10000});
  await game.act({keys:[],duration_ms:1000},{name:'wasp-defeated',when:()=>true});
  const final=await save('after-parent-death');
  game.check('dead Queen cannot replenish her camp',members(final).every(e=>remaining.some(p=>p.id===e.id))&&!final.diagnostic.entities.some(e=>e.camp===queen.camp&&e.kind===0x947));
  game.check('Queen defeat XP is not awarded twice',final.view.character.experience===dead.view.character.experience);
  game.check('same living prepared Wizard completes encounter',final.view.lifecycle.alive&&final.view.session.character===begin.view.session.character);
}
