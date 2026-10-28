import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:5,seed:12351,world:1,equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,10,0,30,10,0,0]};
let leader;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'werewolf-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor}),root=join(process.env.QA_RESULTS_DIR,'native');
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  leader=map.entities.find(e=>e.kind===0x623&&Array.from({length:7},(_,i)=>!blocked.has([Math.floor(e.pos.x)-i,Math.floor(e.pos.y)].join(','))).every(Boolean));
  assert(leader,'No natural Desert Werewolf with a clear approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-family.json'),JSON.stringify(map.entities.filter(e=>e.camp===leader.camp)));
  return {...spec,position:{x:leader.pos.x-3,y:leader.pos.y}};
}
export async function run(game){
  let serial=0,protectedUntil=0;
  async function act(input,capture){
    while(protectedUntil&&(await game.observe()).game_ms<protectedUntil&&(await game.observe()).game_ms+input.duration_ms+1500>=protectedUntil)await game.act({keys:[],duration_ms:100});
    const now=(await game.observe()).game_ms;
    if(now>=protectedUntil){
      const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
      await writeFile(path,JSON.stringify({id:'werewolf-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
      execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);protectedUntil=now+60000;
    }
    return game.act(input,capture);
  }
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===leader.camp&&[0x620,0x622].includes(e.kind));
  const begin=await save('arrival');
  game.check('natural MidSand leader has selected HP and size',entity(begin,leader.id).hp===320&&entity(begin,leader.id).size===135);
  for(const kind of [0x620,0x622])game.check(kind+': one to three natural initial Wargs',family(begin).filter(e=>e.kind===kind).length>=1&&family(begin).filter(e=>e.kind===kind).length<=3);
  await game.act({keys:[],duration_ms:2000,target:{entity:leader.id,camera_angle:-Math.PI/4,fire:false}});
  const exposed=await save('exposed');
  game.check('unprotected native Wizard takes encounter damage',exposed.view.character.hp<begin.view.character.hp);
  game.check('level-five native player receives a visible Werewolf quest',entity(exposed,exposed.view.quest)?.kind===0x623);
  const samples=[];
  for(let n=0;n<40;n++){await act({keys:[],duration_ms:500},n===39?{name:'werewolf-unharmed',when:()=>true}:undefined);samples.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'attacks.json'),JSON.stringify(samples));
  game.check('Werewolf activates and wanders',samples.some(s=>{const e=entity(s,leader.id);return e.activated!==null&&Math.hypot(e.pos.x-leader.pos.x,e.pos.y-leader.pos.y)>.2;}));
  for(const kind of [0x620,0x622])game.check(kind+': living population reaches its reconstructed cap of three',family(samples.at(-1)).filter(e=>e.kind===kind).length===3&&samples.every(s=>family(s).filter(e=>e.kind===kind).length<=3));
  const shots=samples.flatMap(s=>s.view.projectiles),ids=kind=>new Set(samples.flatMap(family).filter(e=>e.kind===kind).map(e=>e.id));
  game.check('unharmed Werewolf fires selected speed/lifetime',shots.some(p=>p.owner===leader.id&&p.speed===60&&p.lifetime_ms===2000));
  for(const [kind,lifetime] of [[0x620,600],[0x622,2000]])game.check(kind+': Wargs move and fire selected speed/lifetime',samples.some(s=>family(s).some(e=>e.kind===kind&&family(begin).some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.1)))&&shots.some(p=>ids(kind).has(p.owner)&&p.speed===60&&p.lifetime_ms===lifetime));
  async function shoot(id,stop){
    for(let n=0;n<160;n++){
      const s=await game.inspect(),e=entity(s,id);if(!e||stop?.(e))return;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,a=-Math.PI/4,x=dx*Math.cos(a)+dy*Math.sin(a),y=-dx*Math.sin(a)+dy*Math.cos(a);
      const approach=Math.hypot(dx,dy)>5;
      const keys=approach?[...(Math.abs(x)>Math.abs(y)*.5?[x>0?'d':'a']:[]),...(Math.abs(y)>Math.abs(x)*.5?[y>0?'s':'w']:[])]:[];
      await act({keys,duration_ms:250,target:{entity:id,camera_angle:a,fire:!approach}});
    }
    assert.fail('Native shots failed to reach target outcome: '+id);
  }
  const victim=family(samples.at(-1)).find(e=>e.kind===0x620),beforeWarg=(await game.observe()).character.experience;
  await shoot(victim.id);await act({keys:[],duration_ms:500});
  game.check('native shooting defeats Warg and earns XP',!entity(await game.inspect(),victim.id)&&(await game.observe()).character.experience>beforeWarg);
  for(let n=0;n<22;n++)await act({keys:[],duration_ms:500});
  const replaced=await save('replenished');
  game.check('living Werewolf replenishes a defeated Tawny Warg within its cap',family(replaced).filter(e=>e.kind===0x620).length===3&&family(replaced).some(e=>e.kind===0x620&&!ids(0x620).has(e.id)));
  // Current reconstruction is not target-era acceptance: Kithio instead uses <75% HP and projectile 0 in both phases.
  // https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Midland.cs#L815-L846
  await shoot(leader.id,e=>e.hp<200);
  await act({keys:[],duration_ms:600},{name:'werewolf-enraged',when:()=>true});
  const rage=await save('enraged');
  game.check('native damage triggers reconstructed rage and size 150',entity(rage,leader.id)?.hp<200&&entity(rage,leader.id).stage===1&&entity(rage,leader.id).size===150);
  const enraged=[];
  for(let n=0;n<8;n++){await act({keys:[],duration_ms:250});enraged.push(await game.observe());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'rage-attacks.json'),JSON.stringify(enraged));
  game.check('enraged Werewolf fires faster selected projectile',enraged.flatMap(s=>s.projectiles).some(p=>p.owner===leader.id&&p.speed===65&&p.lifetime_ms===2000));
  const xp=(await game.observe()).character.experience;
  await shoot(leader.id);await act({keys:[],duration_ms:600},{name:'werewolf-defeated',when:()=>true});
  const dead=await save('defeated');
  game.check('native shooting defeats Werewolf and commits eligible XP',!entity(dead,leader.id)&&dead.view.character.experience>=xp+52);
  await act({keys:[],duration_ms:6000});await act({keys:[],duration_ms:6000});
  const end=await save('finished');
  game.check('dead Werewolf cannot replenish Wargs',!entity(end,leader.id)&&family(end).every(e=>family(dead).some(p=>p.id===e.id)));
  game.check('defeat reward is not repeated',end.view.character.experience===dead.view.character.experience);
  game.check('same prepared living character completes encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
}
