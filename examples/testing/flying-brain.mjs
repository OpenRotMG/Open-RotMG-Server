import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const output=name=>join(process.env.QA_RESULTS_DIR,name);
let root;
export async function prepare(api){
  const spec={class:0x30e,level:20,seed:846,world:1,equipment:[0xb08,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,30,10,0,0]};
  const report=JSON.parse(await readFile(output('native/result.json'),'utf8'));
  const info=await api({op:'world_info',world:1}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'brain-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+(await readFile(output('native/state/control.token'),'utf8')).trim()}});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const kinds=new Set(JSON.parse(execFileSync('python3',['-c',"import json,xml.etree.ElementTree as E; print(json.dumps([int(e.attrib['type'],0) for e in E.parse('data/objects.xml').getroot() if e.find('Enemy') is not None]))"],{encoding:'utf8'})));
  const candidates=map.entities.filter(e=>e.kind===0x657).map(e=>({entity:e,clearance:Math.min(...map.entities.filter(p=>p.id!==e.id&&kinds.has(p.kind)).map(p=>Math.hypot(p.pos.x-e.pos.x,p.pos.y-e.pos.y)))})).filter(({entity:e})=>{
    for(let dx=-8;dx<=8;dx++)for(let dy=-4;dy<=4;dy++)if(blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))return false;
    return true;
  }).sort((a,b)=>b.clearance-a.clearance);
  assert(candidates.length,'No naturally placed Flying Brain has a clear observation corridor');root=candidates[0].entity;
  await writeFile(output('natural-placement.json'),JSON.stringify({world:1,layout:info.realm_layout,root,clearance:candidates[0].clearance,candidates}));
  return {...spec,position:{x:root.pos.x-5,y:root.pos.y}};
}
export async function run(game){
  const samples=[],shots=new Map();
  const enemy=s=>s.diagnostic.entities.find(e=>e.id===root.id);
  async function act(input,capture){
    let pending,error;
    const record=s=>{samples.push(s);for(const p of s.view.projectiles.filter(p=>p.owner===root.id))shots.set(p.id,p);};
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;const s=await game.inspect();record(s);return s;
  }
  try{
    const arrival=await game.inspect();
    game.check('natural mountain Flying Brain admitted at selected 1000 HP',enemy(arrival)?.hp===1000&&arrival.view.session.world===1);
    await act({keys:[],duration_ms:1100},{name:'flying-brain-arrival',when:()=>true});
    const hurt=await game.inspect();
    game.check('unprotected native Wizard takes encounter damage',hurt.view.character.hp<arrival.view.character.hp&&hurt.view.lifecycle.alive);
    const path=output('protection.json');
    await writeFile(path,JSON.stringify({id:'brain-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),output('native/native.json'),path]);
    await act({keys:[],duration_ms:3500},{name:'flying-brain-rings',when:()=>true});
    await act({keys:['d','w'],duration_ms:600});
    await act({keys:[],duration_ms:2000});
    const combat=await game.inspect();
    game.check('natural Flying Brain activates and moves',samples.some(s=>enemy(s)?.activated!=null&&Math.hypot(enemy(s).pos.x-root.pos.x,enemy(s).pos.y-root.pos.y)>.2));
    game.check('native Pink Bolts use selected speed 12 tiles/s and 1.8s lifetime',shots.size>=15&&[...shots.values()].every(p=>p.speed===120&&p.lifetime_ms===1800));
    const volleys=new Map();
    for(const p of shots.values()){if(!volleys.has(p.start_time_ms))volleys.set(p.start_time_ms,[]);volleys.get(p.start_time_ms).push(p);}
    const complete=[...volleys.values()].filter(v=>v.length===5);
    // Shoot's centered 72-degree fan includes the target bearing: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Mountain.cs
    game.check('native volley is a five-shot ring at 72-degree gaps',complete.length>=3&&complete.every(v=>{
      const a=v.map(p=>(p.angle%(2*Math.PI)+2*Math.PI)%(2*Math.PI)).sort((a,b)=>a-b);
      return a.every((v,i)=>Math.abs((i===4?a[0]+2*Math.PI:a[i+1])-v-2*Math.PI/5)<.001);
    }));
    game.check('stationary native player is on a ring spoke',samples.some(s=>{
      const e=enemy(s);if(!e)return false;
      return s.view.projectiles.some(p=>p.owner===root.id&&Math.abs(Math.atan2(Math.sin(p.angle-Math.atan2(s.view.position.y-p.start.y,s.view.position.x-p.start.x)),Math.cos(p.angle-Math.atan2(s.view.position.y-p.start.y,s.view.position.x-p.start.x))))<.025);
    }));
    const times=complete.map(v=>v[0].start_time_ms).sort((a,b)=>a-b);
    game.check('consecutive native rings have 500ms cadence',times.slice(1).filter((t,i)=>Math.abs(t-times[i]-500)<=50).length>=3);
    for(let n=0;n<80&&enemy(await game.inspect());n++){
      const s=await game.inspect(),e=enemy(s),dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,approach=Math.hypot(dx,dy)>5;
      const keys=approach?[...(Math.abs(x)>Math.abs(y)*.5?[x>0?'d':'a']:[]),...(Math.abs(y)>Math.abs(x)*.5?[y>0?'s':'w']:[])]:[];
      await act({keys,duration_ms:200,target:{entity:root.id,camera_angle:-Math.PI/4,fire:!approach}});
    }
    await act({keys:[],duration_ms:500},{name:'flying-brain-defeated',when:()=>true});
    const dead=await game.inspect();
    game.check('ordinary native staff defeats natural Flying Brain and earns XP',!enemy(dead)&&dead.view.character.experience>=combat.view.character.experience+100);
    await act({keys:[],duration_ms:3000});const end=await game.inspect();
    game.check('defeated Flying Brain stays absent without repeated XP',!enemy(end)&&end.view.character.experience===dead.view.character.experience);
    game.check('same prepared living Wizard remains in the Realm',end.view.lifecycle.alive&&end.view.session.world===1&&end.view.session.character===arrival.view.session.character);
    await writeFile(output('outcomes.json'),JSON.stringify({arrival,hurt,combat,dead,end,volleys:[...volleys.values()]}));
  }finally{await writeFile(output('combat.json'),JSON.stringify({root,samples,shots:[...shots.values()]}));}
}
