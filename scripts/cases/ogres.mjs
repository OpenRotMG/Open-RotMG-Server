import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:832,world:1,equipment:[0xa9f,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,50,75,40,60]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
let king;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'ogre-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root(),'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root(),'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  for(const e of map.entities.filter(e=>e.kind===0x645))for(let dx=-2;dx<=2;dx++)for(let dy=-2;dy<=2;dy++){
    const position={x:Math.floor(e.pos.x)+.5+dx,y:Math.floor(e.pos.y)+.5+dy};
    if(Math.hypot(position.x-e.pos.x,position.y-e.pos.y)<1.5)continue;
    if([-1,0,1].every(x=>[-1,0,1].every(y=>!blocked.has([Math.floor(position.x+x),Math.floor(position.y+y)].join(','))))){
      king=e;await writeFile(join(process.env.QA_RESULTS_DIR,'natural-camp.json'),JSON.stringify(map.entities.filter(m=>m.camp===e.camp)));return {...spec,position};
    }
  }
  throw Error('No naturally allocated Ogre King camp with a clear approach');
}
export async function run(game){
  const samples=[],shots=new Map();let serial=0,protectedUntil=0;
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===king.camp);
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'ogre-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root(),'native.json'),path]);protectedUntil=now+60000;
  }
  async function step(input,capture){
    await game.act(input,capture);const s=await game.inspect();samples.push(s);for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);return s;
  }
  async function act(input,capture){
    await protect();while(protectedUntil-(await game.observe()).game_ms<input.duration_ms+500){await step({keys:[],duration_ms:100});await protect();}
    return step(input,capture);
  }
  const begin=await save('arrival'),counts=[[0x645,1,500],[0x642,2,300],[0x643,4,270],[0x644,2,300]];
  game.check('natural HighForest camp has selected HP and current nine-actor population',counts.every(([k,n,hp])=>family(begin).filter(e=>e.kind===k&&e.hp===hp).length===n));
  await protect();
  await act({keys:[],duration_ms:2500});
  await act({keys:[],duration_ms:800},{name:'ogres-arrival',when:()=>true});
  for(let n=0;n<48;n++)await act({keys:[],duration_ms:250});
  await act({keys:[],duration_ms:800},{name:'ogres-combat',when:()=>true});
  // This exercises today's reconstruction, not Kithio's different King cycle: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Highland.cs#L70-L162
  for(const [kind,speed,lifetime] of [[0x645,100,400],[0x642,67,1500],[0x643,100,400],[0x644,50,2200]]){
    const ids=new Set(family(begin).filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': activates and sends selected projectile to native client',samples.some(s=>family(s).some(e=>ids.has(e.id)&&e.activated!==null))&&[...shots.values()].some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime));
    game.check(kind+': moves in its natural camp',samples.some(s=>family(s).some(e=>ids.has(e.id)&&family(begin).some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.2))));
  }
  game.check('current one-shot population does not grow or replace actors',samples.every(s=>family(s).length===9&&family(s).every(e=>family(begin).some(p=>p.id===e.id))));
  await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples,shots:[...shots.values()]}));
  async function defeat(id){
    const xp=(await game.observe()).character.experience;
    for(let n=0;n<100;n++){
      const s=await game.inspect(),e=family(s).find(e=>e.id===id);if(!e)break;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      if(Math.hypot(dx,dy)>4||!s.view.objects.some(o=>o.id===id&&o.hp>0))await act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:350});
      else await act({keys:[],duration_ms:350,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
      await act({keys:[],duration_ms:250});
    }
    const dead=await save('defeated-'+id);game.check(id+': ordinary native staff defeats actor and earns XP',!family(dead).some(e=>e.id===id)&&dead.view.character.experience>xp);return dead;
  }
  const dead=await defeat(king.id),survivors=family(dead).map(e=>e.id);
  game.check('ordinary followers survive King defeat',survivors.length>0);
  for(const kind of [0x642,0x643,0x644]){
    const s=await game.inspect(),victim=family(s).find(e=>e.kind===kind);
    if(victim)await defeat(victim.id);
    else game.check(kind+': staff collateral already defeated original role',family(begin).filter(e=>e.kind===kind).every(e=>!family(s).some(p=>p.id===e.id))&&s.view.character.experience>begin.view.character.experience);
  }
  const cleared=await save('rewards');await act({keys:[],duration_ms:10000});
  await act({keys:[],duration_ms:800},{name:'ogres-defeated',when:()=>true});const end=await save('end');
  game.check('King stays dead and cannot duplicate XP',!family(end).some(e=>e.kind===0x645)&&end.view.character.experience===cleared.view.character.experience);
  game.check('dead camp cannot replace defeated escorts',family(end).every(e=>family(cleared).some(p=>p.id===e.id)));
  game.check('same prepared living native Wizard completes encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
}
