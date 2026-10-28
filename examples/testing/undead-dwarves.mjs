import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:829,world:1,equipment:[0xa9f,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,50,75,40,60]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
let god;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'undead-dwarf-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root(),'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root(),'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  god=map.entities.find(e=>e.kind===0x640&&[-9,-8,-7,-6,-5,-4,-3,-2,-1,0].every(dx=>[-1,0,1].every(dy=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))));
  assert(god,'No natural HighPlains camp with a clear approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-camp.json'),JSON.stringify(map.entities.filter(e=>e.camp===god.camp)));
  return {...spec,position:{x:god.pos.x-8,y:god.pos.y}};
}
export async function run(game){
  const samples=[],shots=new Map();let protectedUntil=0,serial=0;
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===god.camp),entity=(s,id)=>family(s).find(e=>e.id===id);
  const record=s=>{samples.push(s);for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);};
  async function step(input,capture){
    let pending=null,error;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;const s=await game.inspect();record(s);return s;
  }
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    const path=join(process.env.QA_RESULTS_DIR,'protection.json');
    await writeFile(path,JSON.stringify({id:'undead-dwarf-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root(),'native.json'),path]);protectedUntil=now+60000;
  }
  async function act(input,capture){
    await protect();while(protectedUntil-(await game.observe()).game_ms<input.duration_ms+500){await step({keys:[],duration_ms:100});await protect();}
    return step(input,capture);
  }
  const begin=await save('arrival');
  game.check('natural HighPlains camp has current selected HP and initial population',[[0x640,1,800],[0x63e,1,400],[0x63f,1,250],[0x63a,2,250],[0x63b,2,260],[0x63c,2,210]].every(([k,n,hp])=>family(begin).filter(e=>e.kind===k&&e.hp===hp).length===n));
  await step({keys:[],duration_ms:500},{name:'undead-dwarves-arrival',when:()=>true});
  for(let n=0;n<12&&!samples.some(s=>(s.view.character.conditions&1024)!==0);n++)await step({keys:[],duration_ms:250});
  await save('exposed');
  game.check('ordinary enemy projectiles damage the native Wizard',samples.some(s=>s.view.character.hp<begin.view.character.hp));
  game.check('zero-damage Blue Star applies Confused through ordinary native collision',samples.some(s=>(s.view.character.conditions&1024)!==0));
  await protect();
  for(let n=0;n<24;n++)await act({keys:[],duration_ms:500});
  await act({keys:[],duration_ms:500},{name:'undead-dwarves-combat',when:()=>true});
  await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples,shots:[...shots.values()]}));
  for(const [kind,speed,lifetime] of [[0x640,70,1700],[0x63e,100,600],[0x63f,70,1700],[0x63a,60,1200],[0x63b,120,400],[0x63c,50,2400]]){
    const ids=new Set(samples.flatMap(family).filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': natural actor activates and moves',samples.some(s=>family(s).some(e=>ids.has(e.id)&&e.activated!==null&&family(begin).some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.2))));
    game.check(kind+': native client receives selected projectile',[...shots.values()].some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime));
  }
  // Spawn ownership and birth counters differ from our nested reconstruction: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/behaviors/Spawn.cs
  const caps=s=>[[0x640,1],[0x63e,1],[0x63f,1],[0x63a,3],[0x63b,3],[0x63c,3]].every(([k,n])=>family(s).filter(e=>e.kind===k).length<=n);
  game.check('current bounded population caps hold',samples.every(caps));
  const initialIds=new Set(family(begin).map(e=>e.id));
  game.check('living nested King fills all three escort roles',[[0x63a,3],[0x63b,3],[0x63c,3]].every(([k,n])=>family(samples.at(-1)).filter(e=>e.kind===k).length===n)&&family(samples.at(-1)).some(e=>!initialIds.has(e.id)));
  async function defeat(id){
    const before=await game.inspect();
    for(let n=0;n<140&&entity(await game.inspect(),id);n++)await act({keys:[],duration_ms:150,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
    const after=await game.inspect();
    game.check(id+': ordinary native staff defeats actor and earns XP',!entity(after,id)&&after.view.character.experience>before.view.character.experience);
    return after;
  }
  const king=family(await game.inspect()).find(e=>e.kind===0x63e);assert(king);
  const deadKing=await defeat(king.id),old=new Set(family(deadKing).map(e=>e.id));
  for(let n=0;n<15&&!family(await game.inspect()).some(e=>e.kind===0x63e&&e.id!==king.id);n++)await act({keys:[],duration_ms:1000});
  const replaced=await save('king-replaced');
  game.check('living God replaces a defeated King within current cap',entity(replaced,god.id)&&family(replaced).filter(e=>e.kind===0x63e).length===1&&family(replaced).some(e=>e.kind===0x63e&&!old.has(e.id)));
  await defeat(god.id);const dead=await save('god-defeated');
  await act({keys:[],duration_ms:500},{name:'undead-dwarves-defeated',when:()=>true});
  const survivors=family(dead).map(e=>e.id);
  for(let n=0;n<12;n++)await act({keys:[],duration_ms:1000});
  const end=await save('survivors');
  game.check('God defeat preserves ordinary followers',family(end).some(e=>survivors.includes(e.id)));
  game.check('God stays dead and cannot duplicate its XP',!entity(end,god.id)&&end.view.character.experience===dead.view.character.experience);
  game.check('same supplied living native Wizard completes the encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
  await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples,shots:[...shots.values()]}));
}
