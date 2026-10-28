import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const kind=Number(process.env.SPRITE_KIND);
assert([0x624,0x625,0x626].includes(kind),'Set SPRITE_KIND to 0x624, 0x625 or 0x626');
const spec={class:0x30e,level:20,seed:839,world:1,equipment:[0xa9f,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,50,75,0,0]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
let sprite;
export async function prepare(api){
  const world=await api({op:'world_info',world:spec.world}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'sprite-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root(),'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root(),'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  sprite=map.entities.find(e=>e.kind===kind&&[-3,-2,-1,0,1,2,3].every(dx=>[-2,-1,0,1,2].every(dy=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))));
  assert(sprite,'No naturally allocated selected Sprite with clear approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-placement.json'),JSON.stringify({world:spec.world,layout:world.realm_layout,sprite,sprites:map.entities.filter(e=>e.kind>=0x624&&e.kind<=0x626)}));
  return {...spec,position:{x:sprite.pos.x-2,y:sprite.pos.y}};
}
export async function run(game){
  const samples=[],entity=s=>s.diagnostic.entities.find(e=>e.id===sprite.id);
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const begin=await save('arrival');
  game.check('naturally allocated Realm Sprite has selected 100 HP',entity(begin)?.hp===100);
  await game.act({keys:[],duration_ms:500},{name:'sprite-arrival',when:()=>true});
  for(let n=0;n<16;n++){
    const s=await game.inspect();samples.push(s);
    if(s.view.character.hp<begin.view.character.hp&&(kind!==0x625||(s.view.character.conditions&(1<<3))!==0))break;
    await game.act({keys:[],duration_ms:250});
  }
  const hit=await save('unprotected-hit');
  game.check('ordinary enemy fire damages the unprotected native Wizard',hit.view.character.hp<begin.view.character.hp);
  if(kind===0x625)game.check('selected Ice projectile applies Slowed',samples.some(s=>(s.view.character.conditions&(1<<3))!==0));
  const path=join(process.env.QA_RESULTS_DIR,'protection.json');
  await writeFile(path,JSON.stringify({id:'sprite-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root(),'native.json'),path]);
  let pending=null,error;
  const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>samples.push(s)).catch(e=>{error=e;}).finally(()=>{pending=null;});},50);
  try{await game.act({keys:[],duration_ms:6000},{name:'sprite-combat',when:()=>true});}
  finally{clearInterval(timer);if(pending)await pending;await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify(samples));}
  if(error)throw error;
  game.check('natural Sprite activates and wanders',samples.some(s=>{const e=entity(s);return e?.activated!==null&&e&&Math.hypot(e.pos.x-sprite.pos.x,e.pos.y-sprite.pos.y)>.2;}));
  const count=kind-0x622,interval=kind===0x624?300:1000,speed=kind===0x625?80:50,lifetime=kind===0x625?1800:4000;
  const volleys=new Map();
  for(const s of samples)for(const p of s.view.projectiles.filter(p=>p.owner===sprite.id)){
    const key=s.view.time_ms+':'+p.start.x+':'+p.start.y;if(!volleys.has(key))volleys.set(key,new Map());volleys.get(key).set(p.id,p);
  }
  // Kithio supplies fan counts and nominal cadence, not authenticated original timing: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Midland.cs#L15-L50
  game.check('selected projectile speed and lifetime reach the native session',[...volleys.values()].some(v=>[...v.values()].every(p=>p.speed===speed&&p.lifetime_ms===lifetime)));
  game.check('complete reconstructed fan has '+count+' rays spaced seven degrees',[...volleys.values()].some(v=>{const a=[...v.values()].map(p=>p.angle).sort((a,b)=>a-b);return a.length===count&&a.slice(1).every((n,i)=>Math.abs(n-a[i]-7*Math.PI/180)<.001);}));
  const deadlines=[...new Set(samples.map(s=>entity(s)?.next_shot).filter(Number.isFinite))].sort((a,b)=>a-b);
  game.check('reconstructed attack clock advances by '+interval+' ms',deadlines.some((t,i)=>i&&t-deadlines[i-1]===interval));
  const xp=hit.view.character.experience;
  for(let n=0;n<80;n++){
    const s=await game.inspect(),e=entity(s);if(!e)break;
    const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
    if(Math.hypot(dx,dy)>4||!s.view.objects.some(o=>o.id===e.id))await game.act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:200});
    else await game.act({keys:[],duration_ms:250,target:{entity:e.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const dead=await save('defeated');
  game.check('ordinary native staff fire defeats Sprite and earns XP',!entity(dead)&&dead.view.character.experience>xp);
  await game.act({keys:[],duration_ms:2000},{name:'sprite-defeated',when:()=>true});
  const end=await save('end');
  game.check('dead Sprite cannot award duplicate XP',!entity(end)&&end.view.character.experience===dead.view.character.experience);
  game.check('same living prepared Wizard completes encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
}
