import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:835,world:1,equipment:[0xa9f,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,50,75,40,60]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
let god;
export async function prepare(api){
  const report=JSON.parse(await readFile(join(root(),'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root(),'state/control.token'),'utf8')).trim()};
  for(const world of [-3,1]){
    const info=await api({op:'world_info',world});assert.equal(info.name,'Realm');const [x,y]=info.spawn_points[0];
    const scout=await api({op:'spawn',spec:{...spec,world,guid:'flayer-scout-'+world,position:{x,y},controller:'direct',profile:null}});
    const state=await api({op:'inspect',actor:scout.result.actor});
    const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
    const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
    await writeFile(join(process.env.QA_RESULTS_DIR,'natural-map-'+world+'.json'),JSON.stringify(map));
    for(const e of map.entities.filter(e=>e.kind===0x649))for(let dx=-9;dx<=9;dx++)for(let dy=-9;dy<=9;dy++){
      const position={x:Math.floor(e.pos.x)+.5+dx,y:Math.floor(e.pos.y)+.5+dy},distance=Math.hypot(position.x-e.pos.x,position.y-e.pos.y);
      if(distance<8||distance>9)continue;
      if(Array.from({length:21},(_,i)=>({x:e.pos.x+(position.x-e.pos.x)*i/20,y:e.pos.y+(position.y-e.pos.y)*i/20})).every(p=>!blocked.has([Math.floor(p.x),Math.floor(p.y)].join(',')))){
        god=e;await writeFile(join(process.env.QA_RESULTS_DIR,'natural-camp.json'),JSON.stringify({world,layout:info.realm_layout,members:map.entities.filter(m=>m.camp===e.camp)}));return {...spec,world,position};
      }
    }
  }
  throw Error('Neither initial live Realm (-3 and 1) has a natural Flayer God camp with a clear approach');
}
export async function run(game){
  const samples=[],shots=new Map();let serial=0,protectedUntil=0;
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===god.camp);
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
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
    await writeFile(path,JSON.stringify({id:'flayer-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root(),'native.json'),path]);protectedUntil=now+60000;
  }
  async function act(input,capture){
    await protect();while(protectedUntil-(await game.observe()).game_ms<input.duration_ms+500){await step({keys:[],duration_ms:100});await protect();}
    return step(input,capture);
  }
  async function defeat(id){
    const xp=(await game.observe()).character.experience;
    for(let n=0;n<100;n++){
      const s=await game.inspect(),e=family(s).find(e=>e.id===id);if(!e)break;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      if(Math.hypot(dx,dy)>4||!s.view.objects.some(o=>o.id===id&&o.hp>0))await act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:250});
      else await act({keys:[],duration_ms:250,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
      await act({keys:[],duration_ms:250});
    }
    const dead=await save('defeated-'+id);game.check(id+': ordinary native staff defeats actor and earns XP',!family(dead).some(e=>e.id===id)&&dead.view.character.experience>xp);return dead;
  }
  try{
    const begin=await save('arrival');
    game.check('natural HighSand camp has current initial population and selected HP',[[0x649,1,1400],[0x647,3,140],[0x648,2,280]].every(([k,n,hp])=>family(begin).filter(e=>e.kind===k&&e.hp===hp).length===n));
    await act({keys:[],duration_ms:1000},{name:'flayers-arrival',when:()=>true});
    for(let n=0;n<24;n++)await act({keys:[],duration_ms:500});
    await act({keys:[],duration_ms:1000},{name:'flayers-combat',when:()=>true});
    for(const [kind,speed,lifetime] of [[0x647,50,2700],[0x648,80,1500],[0x649,50,2200],[0x649,70,2200]]){
      const ids=new Set(samples.flatMap(family).filter(e=>e.kind===kind).map(e=>e.id));
      game.check(kind+'/'+speed+': activates and sends selected projectile during native combat',samples.some(s=>family(s).some(e=>ids.has(e.id)&&e.activated!==null))&&[...shots.values()].some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime));
    }
    game.check('ordinary zero-damage star applies Quiet and empties mana through Invulnerable',samples.some(s=>(s.view.character.conditions&2)!==0&&s.view.character.mp===0));
    for(const kind of [0x647,0x648,0x649])game.check(kind+': original actor moves in its natural camp',samples.some(s=>family(s).some(e=>e.kind===kind&&family(begin).some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.2))));
    // These are today's live-child caps, not Kithio's lifetime Spawn counters: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/behaviors/Spawn.cs
    game.check('current live-child caps hold',samples.every(s=>[[0x649,1],[0x647,4],[0x648,3]].every(([k,n])=>family(s).filter(e=>e.kind===k).length<=n)));
    game.check('living God fills current four-Flayer three-Veteran caps',[[0x647,4],[0x648,3]].every(([k,n])=>family(samples.at(-1)).filter(e=>e.kind===k).length===n));
    const victim=family(await game.inspect()).find(e=>e.kind===0x648);assert(victim);
    await defeat(victim.id);
    const old=new Set(family(await game.inspect()).map(e=>e.id));
    for(let n=0;n<12&&!family(await game.inspect()).some(e=>e.kind===0x648&&!old.has(e.id));n++)await act({keys:[],duration_ms:1000});
    const replaced=await save('veteran-replaced');
    game.check('living God replaces a defeated Veteran within current cap',family(replaced).some(e=>e.id===god.id)&&family(replaced).filter(e=>e.kind===0x648).length===3&&family(replaced).some(e=>e.kind===0x648&&!old.has(e.id)));
    await defeat(god.id);
    await act({keys:[],duration_ms:2000});
    const dead=await save('god-defeated'),survivors=family(dead).map(e=>e.id);
    await act({keys:[],duration_ms:10000});
    await act({keys:[],duration_ms:1000},{name:'flayers-defeated',when:()=>true});
    const end=await save('end');
    game.check('God remains dead without duplicate XP',!family(end).some(e=>e.kind===0x649)&&end.view.character.experience===dead.view.character.experience);
    game.check('ordinary followers survive God defeat without further births',family(end).length>0&&family(end).every(e=>survivors.includes(e.id)));
    game.check('same supplied living native Wizard completes encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
  }finally{await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples,shots:[...shots.values()]}));}
}
