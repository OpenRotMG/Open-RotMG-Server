import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:843,world:1,equipment:[0xb08,0xb24,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,50,75,0,0]};
let medusa;
export async function prepare(api){
  const info=await api({op:'world_info',world:1}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'medusa-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor}),root=join(process.env.QA_RESULTS_DIR,'native');
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()}});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const candidates=map.entities.filter(e=>e.kind===0x654&&[-1,0,1].every(dx=>[-1,0,1].every(dy=>!blocked.has([Math.floor(e.pos.x)-5+dx,Math.floor(e.pos.y)+dy].join(',')))));
  const crowd=e=>map.entities.filter(o=>o.id!==e.id&&o.hp>0&&Math.hypot(o.pos.x-e.pos.x+5,o.pos.y-e.pos.y)<12).length;
  medusa=candidates.sort((a,b)=>crowd(a)-crowd(b))[0];
  assert(medusa,'No natural Medusa with a walkable approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-placement.json'),JSON.stringify({world:info,medusa,medusas:map.entities.filter(e=>e.kind===0x654)}));
  return {...spec,position:{x:medusa.pos.x-5,y:medusa.pos.y}};
}
export async function run(game){
  const samples=[];let serial=0,protectedUntil=0;
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const enemy=s=>s.diagnostic.entities.find(e=>e.id===medusa.id);
  async function act(input,capture){
    const now=(await game.observe()).game_ms;
    if(now>=protectedUntil){
      const path=join(process.env.QA_RESULTS_DIR,'protection.json');
      await writeFile(path,JSON.stringify({id:'medusa-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
      execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);protectedUntil=now+60000;
    }
    let pending=null,error;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>samples.push(s)).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
    try{return await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;if(error)throw error;}
  }
  try{
    const begin=await save('arrival');
    game.check('naturally allocated Medusa starts with selected 2000 HP and size 100',enemy(begin)?.hp===2000&&enemy(begin).size===100);
    await game.act({keys:[],duration_ms:1000},{name:'medusa-arrival',when:()=>true});
    const hurt=await save('unprotected');
    game.check('unprotected native character takes encounter damage',hurt.view.character.hp<begin.view.character.hp&&hurt.view.lifecycle.alive);
    await act({keys:[],duration_ms:9000},{name:'medusa-attacks',when:()=>true});
    const combat=await save('combat');
    game.check('Medusa activates and moves in the Realm',enemy(combat)?.activated!==null&&samples.some(s=>{const e=enemy(s);return e&&Math.hypot(e.pos.x-medusa.pos.x,e.pos.y-medusa.pos.y)>.2;}));
    // The period page independently lists the same fan and grenade: https://web.archive.org/web/20120522023605id_/http://forums.wildshadow.com/wiki/realm-mad-god/medusa
    const shots=samples.flatMap(s=>s.view.projectiles).filter(p=>p.owner===medusa.id);
    game.check('native view receives Green Bolts with selected speed and lifetime',shots.some(p=>p.speed===70&&p.lifetime_ms===2000));
    game.check('native view receives a five-shot fan at ten-degree spacing',samples.some(s=>{
      const groups=new Map();
      for(const p of s.view.projectiles.filter(p=>p.owner===medusa.id)){const key=p.start.x+':'+p.start.y+':'+p.start_time_ms;if(!groups.has(key))groups.set(key,[]);groups.get(key).push(p.angle);}
      return [...groups.values()].some(a=>a.length===5&&a.sort((a,b)=>a-b).every((v,i)=>i===0||Math.abs(v-a[i-1]-Math.PI/18)<.0001));
    }));
    const xp=combat.view.character.experience;
    for(let n=0;n<65;n++){
      const s=await game.inspect(),e=enemy(s);if(!e)break;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      if(Math.hypot(dx,dy)>5||!s.view.objects.some(o=>o.id===medusa.id&&o.hp>0))await act({keys:[Math.abs(x)>.2?(x>0?'d':'a'):null,Math.abs(y)>.2?(y>0?'s':'w'):null].filter(Boolean),duration_ms:200});
      else await act({keys:[],duration_ms:500,target:{entity:medusa.id,camera_angle:-Math.PI/4,fire:true}});
    }
    await act({keys:[],duration_ms:500},{name:'medusa-defeated',when:()=>true});
    const dead=await save('defeated');
    game.check('ordinary native staff defeats the same Medusa and earns XP',!enemy(dead)&&dead.view.character.experience>=xp+200);
    await act({keys:[],duration_ms:3000});
    const end=await save('end');
    game.check('defeated Medusa cannot award XP twice',!enemy(end)&&end.view.character.experience===dead.view.character.experience);
    game.check('same prepared living character retains supplied equipment',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character&&JSON.stringify(end.view.character.items)===JSON.stringify(begin.view.character.items));
  }finally{await writeFile(join(process.env.QA_RESULTS_DIR,'combat-samples.json'),JSON.stringify(samples));}
}
