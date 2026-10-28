import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const spec={class:0x30e,level:20,seed:834,world:1,equipment:[0xa9f,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
let snake,mapFor;
export async function prepare(api){
  const info=await api({op:'world_info',world:1}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'snake-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root(),'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root(),'state/control.token'),'utf8')).trim()};
  mapFor=async player=>{const response=await fetch(report.origin+'/dev/map?player='+player,{headers});assert(response.ok);return response.json();};
  const map=await mapFor(state.diagnostic.player),blocked=new Set(map.blocked.map(p=>p.join(',')));
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-map.json'),JSON.stringify(map));
  for(const e of map.entities.filter(e=>e.kind===0x602))for(const dx of [-3,3]){
    const position={x:Math.floor(e.pos.x)+.5+dx,y:Math.floor(e.pos.y)+.5};
    if([-1,0,1].every(x=>[-1,0,1].every(y=>!blocked.has([Math.floor(position.x+x),Math.floor(position.y+y)].join(','))))){snake=e;return {...spec,position};}
  }
  throw Error('No naturally populated Shore Snake with a clear approach');
}
export async function run(game){
  const samples=[],shots=new Map();
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const begin=await save('arrival');
  game.check('natural Shore Snake has selected five HP',begin.diagnostic.entities.some(e=>e.id===snake.id&&e.kind===0x602&&e.hp===5));
  await game.act({keys:[],duration_ms:800},{name:'snake-arrival',when:()=>true});
  for(let n=0;n<40;n++){
    await game.act({keys:[],duration_ms:200});const s=await game.inspect();samples.push(s);
    for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);
  }
  await game.act({keys:[],duration_ms:800},{name:'snake-combat',when:()=>true});
  game.check('natural Snake activates and wanders',samples.some(s=>s.diagnostic.entities.some(e=>e.id===snake.id&&e.activated!==null&&Math.hypot(e.pos.x-snake.pos.x,e.pos.y-snake.pos.y)>.2)));
  // Do not mistake this bounded movement check for acceptance of Kithio's local reproduction: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Shore.cs#L38-L47
  game.check('selected Green Magic appears during native combat',[...shots.values()].some(p=>p.owner===snake.id&&p.speed===40&&p.lifetime_ms===2000));
  const ready=await game.inspect(),distance=e=>Math.hypot(e.pos.x-ready.view.position.x,e.pos.y-ready.view.position.y);
  const victim=ready.diagnostic.entities.filter(e=>e.kind===0x602&&ready.view.objects.some(o=>o.id===e.id)).sort((a,b)=>distance(a)-distance(b))[0];
  assert(victim&&distance(victim)<=4,'No observed natural Snake within native firing distance');
  const xp=ready.view.character.experience;
  for(let n=0;n<40;n++){
    const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===victim.id);if(!e)break;
    const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
    if(Math.hypot(dx,dy)>4||!s.view.objects.some(o=>o.id===e.id))await game.act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:150});
    else await game.act({keys:[],duration_ms:250,target:{entity:e.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const dead=await save('defeated'),world=await mapFor(dead.diagnostic.player);
  game.check('ordinary native staff defeats an observed natural Snake and earns XP',!world.entities.some(e=>e.id===victim.id)&&dead.view.character.experience>xp);
  await game.act({keys:[],duration_ms:3000},{name:'snake-defeated',when:()=>true});
  const end=await save('end');
  game.check('dead Snake remains absent',!(await mapFor(end.diagnostic.player)).entities.some(e=>e.id===victim.id));
  game.check('same prepared Wizard survives',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
  await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples,shots:[...shots.values()]}));
}
