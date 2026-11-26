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
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-bandits.json'),JSON.stringify(map.entities.filter(e=>e.id===leader.id||e.parent===leader.id)));
  return {...spec,position:{x:leader.pos.x-3,y:leader.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  const family=s=>s.diagnostic.entities.filter(e=>(e.id===leader.id||e.parent===leader.id)&&e.kind===0x954),owned=s=>family(s).filter(e=>e.parent===leader.id);
  const begin=await save('arrival');
  // The Leader's XML MakeMinions own its Bandits, so the natural camp places none.
  game.check('natural ShoreSand quest has selected HP and no camp-placed Bandits',entity(begin,leader.id).hp===280&&entity(begin,leader.id).camp==null&&family(begin).every(e=>e.parent===leader.id&&e.hp===50));
  await game.act({keys:[],duration_ms:2000,target:{entity:leader.id,camera_angle:-Math.PI/4,fire:false}});
  const exposed=await save('exposed');
  game.check('unprotected native Wizard takes encounter damage',exposed.view.character.hp<begin.view.character.hp);
  let shielded=0,serial=0;
  // Protection cannot be renewed while active, and loaded acts can overrun: idle out the window, then renew.
  async function act(input,capture){
    let now=(await game.observe()).game_ms;
    while(now<shielded&&shielded-now<input.duration_ms+5000){await game.act({keys:[],duration_ms:Math.min(10000,Math.max(50,shielded-now))});now=(await game.observe()).game_ms;}
    if(now>=shielded){
      const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
      await writeFile(path,JSON.stringify({id:'bandit-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
      execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);shielded=now+60000;
    }
    return game.act(input,capture);
  }
  const samples=[];
  for(let n=0;n<24;n++){await act({keys:[],duration_ms:500},n===12?{name:'bandit-family',when:()=>true}:undefined);samples.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'attacks.json'),JSON.stringify(samples));
  game.check('Leader activates and chases',samples.some(s=>{const e=entity(s,leader.id);return e.activated!==null&&Math.hypot(e.pos.x-leader.pos.x,e.pos.y-leader.pos.y)>.2;}));
  game.check('living Leader keeps its four XML-owned Bandits (actors.xml:4841 maxChildren=4)',owned(samples.at(-1)).length===4);
  const shots=samples.flatMap(s=>s.view.projectiles),ids=new Set(samples.flatMap(family).map(e=>e.id));
  game.check('Leader uses selected 800ms Blade',shots.some(p=>p.owner===leader.id&&p.speed===60&&p.lifetime_ms===800));
  // XML Bandits only shoot projectileId 0 (actors.xml:4887,4898); the selected 1000ms Blade 1 is unused.
  game.check('Bandits use selected 600ms Blade',shots.some(p=>ids.has(p.owner)&&p.speed===60&&p.lifetime_ms===600));
  game.check('XML-owned followers move',samples.some(s=>family(s).some(e=>family(exposed).some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.1))));
  // The XML Leader wanders and its Bandits scatter (actors.xml:4844-4904), so close in before firing.
  async function strike(s,id,ms){
    const e=entity(s,id),dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
    if(Math.hypot(dx,dy)>4||!s.view.objects.some(o=>o.id===id))return act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:200});
    await act({keys:[],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
  }
  async function defeat(id){
    const xp=(await game.observe()).character.experience;
    for(let n=0;n<80;n++){const s=await game.inspect();if(!entity(s,id))break;await strike(s,id,250);}
    const after=await game.inspect();game.check(id+': native shooting defeats target and awards XP',!entity(after,id)&&after.view.character.experience>xp);
  }
  await defeat(family(samples.at(-1))[0].id);
  // The primary comparator disagrees with this reconstruction; do not turn a current-behavior receipt into historical acceptance: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Shore.cs#L112-L162
  for(let n=0;n<80;n++){const s=await game.inspect();if(!(entity(s,leader.id)?.hp>=280*.45))break;await strike(s,leader.id,120);}
  const hurt=await save('low-health');assert(entity(hurt,leader.id),'Leader died before retreat observation');
  game.check('ordinary native shooting reaches the XML meek threshold (actors.xml:4865)',entity(hurt,leader.id).hp<280*.45);
  await act({keys:[],duration_ms:2000},{name:'bandit-retreat',when:()=>true});
  const retreat=await save('retreat'),distance=s=>Math.hypot(entity(s,leader.id).pos.x-s.view.position.x,entity(s,leader.id).pos.y-s.view.position.y);
  game.check('injured Leader retreats from native player',distance(retreat)>distance(hurt)+.2);
  await defeat(leader.id);
  const dead=await save('leader-defeated');
  await act({keys:[],duration_ms:6000});await act({keys:[],duration_ms:6000});
  const orphans=await save('orphans');
  game.check('dead Leader cannot grow or replace the family',!entity(orphans,leader.id)&&family(orphans).every(e=>family(dead).some(p=>p.id===e.id)));
  game.check('Leader XP is not awarded twice',orphans.view.character.experience===dead.view.character.experience);
  await act({keys:[],duration_ms:600},{name:'bandit-defeated',when:()=>true});
  const end=await save('finished');game.check('same prepared living character completes encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
}
