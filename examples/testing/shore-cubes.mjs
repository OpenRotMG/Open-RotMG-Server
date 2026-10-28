import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const kind=Number(process.env.CUBE_KIND);
assert([0x60e,0x60f,0x610].includes(kind),'Set CUBE_KIND to 1550, 1551 or 1552');
const spec={class:0x30e,level:3,seed:842,world:1,equipment:[0xa97,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,20,0,30,20,0,0]};
let cube;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'cube-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor}),root=join(process.env.QA_RESULTS_DIR,'native');
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()}});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  cube=map.entities.find(e=>e.kind===kind&&Array.from({length:9},(_,i)=>i-5).every(dx=>Array.from({length:7},(_,i)=>i-3).every(dy=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))));
  assert(cube,'No naturally allocated cube with a clear native approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-placement.json'),JSON.stringify({world,cube,cubes:map.entities.filter(e=>[0x60e,0x60f,0x610].includes(e.kind))}));
  return {...spec,position:{x:cube.pos.x-3,y:cube.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const entity=s=>s.diagnostic.entities.find(e=>e.id===cube.id);
  const begin=await save('arrival');
  game.check('naturally allocated cube has selected 70 HP and size 80',entity(begin)?.hp===70&&entity(begin).size===80);
  await game.act({keys:[],duration_ms:2000},{name:'cube-arrival',when:()=>true});
  game.check('unprotected native character takes encounter damage',(await game.observe()).character.hp<begin.view.character.hp);
  const protection=join(process.env.QA_RESULTS_DIR,'protection.json');
  await writeFile(protection,JSON.stringify({id:'cube-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),protection]);
  const samples=[];
  for(let n=0;n<32;n++){await game.act({keys:[],duration_ms:250});samples.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify(samples));
  game.check('cube activates and wanders',samples.some(s=>{const e=entity(s);return e&&e.activated!==null&&Math.hypot(e.pos.x-cube.pos.x,e.pos.y-cube.pos.y)>.2;}));
  const shots=samples.flatMap(s=>s.view.projectiles).filter(p=>p.owner===cube.id),speed=kind===0x60f?65:60;
  game.check('native client receives selected cube projectile',shots.some(p=>p.speed===speed&&p.lifetime_ms===2000));
  // Kithio agrees on 2/1/5 shots; radius, prediction and reproduction still differ: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Shore.cs#L168-L199
  const count=kind===0x60e?2:kind===0x60f?1:5;
  game.check('native player view contains the variant volley and angular spacing',samples.some(s=>{
    const groups=new Map();
    for(const p of s.view.projectiles.filter(p=>p.owner===cube.id)){const key=p.start.x+':'+p.start.y;if(!groups.has(key))groups.set(key,[]);groups.get(key).push(p.angle);}
    return [...groups.values()].some(angles=>angles.length===count&&angles.sort((a,b)=>a-b).every((a,i)=>i===0||Math.abs(a-angles[i-1]-(kind===0x60e?10:72)*Math.PI/180)<.0001));
  }));
  await game.act({keys:[],duration_ms:700},{name:'cube-attacks',when:()=>true});
  const xp=(await game.observe()).character.experience;
  for(let n=0;n<80;n++){
    const s=await game.inspect(),e=entity(s);if(!e)break;
    const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
    if(Math.hypot(dx,dy)>4||!s.view.objects.some(o=>o.id===cube.id&&o.hp>0))await game.act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:200});
    else await game.act({keys:[],duration_ms:200,target:{entity:cube.id,camera_angle:-Math.PI/4,fire:true}});
  }
  await game.act({keys:[],duration_ms:500},{name:'cube-defeated',when:()=>true});
  const dead=await save('defeated');
  game.check('ordinary native staff defeats cube and earns at least four XP',!entity(dead)&&dead.view.character.experience>=xp+4);
  await game.act({keys:[],duration_ms:5000});
  const end=await save('finished');
  game.check('defeated cube cannot award XP twice',!entity(end)&&end.view.character.experience===dead.view.character.experience);
  game.check('same prepared living character completes encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
}
