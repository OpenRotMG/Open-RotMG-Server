import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const output=name=>join(process.env.QA_RESULTS_DIR,name);
const spec={class:0x300,level:20,seed:850,world:1,equipment:[0xaff,0xb27,-1,-1],backpack:Array(8).fill(-1),stats:[720,252,50,0,50,75,0,50]};
let root;
export async function prepare(api){
  const info=await api({op:'world_info',world:1}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'leviathan-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(output('native/result.json'),'utf8'));
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+(await readFile(output('native/state/control.token'),'utf8')).trim()}});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const enemies=new Set(JSON.parse(execFileSync('python3',['-c',"import json,xml.etree.ElementTree as E; print(json.dumps([int(e.attrib['type'],0) for e in E.parse('data/objects.xml').getroot() if e.find('Enemy') is not None]))"],{encoding:'utf8'})));
  const candidates=map.entities.filter(e=>e.kind===0x6d8).map(entity=>({entity,clearance:Math.min(...map.entities.filter(e=>e.id!==entity.id&&enemies.has(e.kind)).map(e=>Math.hypot(e.pos.x-entity.pos.x,e.pos.y-entity.pos.y)))})).filter(({entity:e})=>{
    for(let dx=-1;dx<=1;dx++)for(let dy=-1;dy<=1;dy++)if(blocked.has([Math.floor(e.pos.x)-5+dx,Math.floor(e.pos.y)+dy].join(',')))return false;
    return true;
  }).sort((a,b)=>b.clearance-a.clearance);
  assert(candidates.length,'No natural Leviathan has a walkable arrival');root=candidates[0].entity;
  await writeFile(output('natural-placement.json'),JSON.stringify({layout:info.realm_layout,root,candidates}));
  return {...spec,position:{x:root.pos.x-5,y:root.pos.y}};
}
export async function run(game){
  const samples=[];let serial=0,protectedUntil=0;
  const enemy=s=>s.diagnostic.entities.find(e=>e.id===root.id);
  async function act(input,capture){
    const now=(await game.inspect()).view.time_ms;
    if(now>=protectedUntil){
      const path=output('protection.json');
      await writeFile(path,JSON.stringify({id:'leviathan-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
      execFileSync(process.execPath,[resolve('scripts/game.mjs'),output('native/native.json'),path]);protectedUntil=now+60000;
    }
    assert(now+input.duration_ms+3000<protectedUntil,'Prepared protection window exhausted before native defeat');
    const captureAt=Date.now()+1500;
    let pending,error;
    const record=s=>samples.push(s);
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
    try{await game.act(input,capture&&{...capture,when:v=>Date.now()>=captureAt&&capture.when(v)});}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;const state=await game.inspect();record(state);return state;
  }
  try{
    const arrival=await game.inspect();
    game.check('natural mountain Leviathan admitted at selected 2000 HP',arrival.view.session.world===1&&enemy(arrival)?.hp===2000);
    await act({keys:[' '],duration_ms:6000},{name:'leviathan-cloaked',when:v=>!!(v.character.conditions&(1<<12))});
    const combat=await game.inspect();
    game.check('ordinary cloak spends mana and grants invisibility',samples.some(s=>(s.view.character.conditions&(1<<12))&&s.view.character.mp<arrival.view.character.mp));
    const groups=s=>{
      const volleys=new Map();
      for(const p of s.view.projectiles.filter(p=>p.owner===root.id)){
        const born=Math.round((s.view.time_ms-Math.hypot(p.position.x-p.start.x,p.position.y-p.start.y)/(p.speed/10)*1000)/50)*50;
        const key=born+':'+p.start.x+':'+p.start.y;if(!volleys.has(key))volleys.set(key,[]);volleys.get(key).push(p);
      }
      return [...volleys.values()];
    };
    // Build 120 explicitly warns that Leviathans still shoot at cloaked Rogues: https://web.archive.org/web/20120524033343id_/http://forums.wildshadow.com/wiki/realm-mad-god/leviathan
    game.check('Leviathan activates, moves and fires while the only nearby native Rogue is cloaked',samples.some(s=>(s.view.character.conditions&(1<<12))&&enemy(s)?.activated!==null&&Math.hypot(enemy(s).pos.x-root.pos.x,enemy(s).pos.y-root.pos.y)>.2&&groups(s).length>0));
    await act({keys:[],duration_ms:5500},{name:'leviathan-fans',when:()=>true});
    const volleys=samples.flatMap(groups);
    for(const [count,speed,lifetime] of [[9,67,1700],[12,67,1700],[2,85,1800]])game.check('native volley '+count+' has selected speed and lifetime',volleys.some(g=>g.length===count&&g.every(p=>p.speed===speed&&p.lifetime_ms===lifetime)));
    game.check('native blue pairs have fifteen-degree spacing',volleys.some(g=>g.length===2&&g[0].speed===85&&Math.abs(Math.abs(g[0].angle-g[1].angle)-Math.PI/12)<.0001));
    for(let n=0;n<60;n++){
      const s=await game.inspect(),e=enemy(s);if(!e)break;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,distance=Math.hypot(dx,dy),approach=distance>4;
      const keys=approach?[Math.abs(x)>.2?(x>0?'d':'a'):null,Math.abs(y)>.2?(y>0?'s':'w'):null].filter(Boolean):[];
      await act({keys,duration_ms:300,target:{entity:root.id,camera_angle:-Math.PI/4,fire:distance<5}});
    }
    await act({keys:[],duration_ms:3000},{name:'leviathan-defeated',when:()=>true});
    const dead=await game.inspect(),lastAlive=samples.findLast(s=>enemy(s));
    game.check('ordinary native dagger defeats the same Leviathan and earns at least capped 195 XP',!enemy(dead)&&lastAlive&&dead.view.character.experience>=lastAlive.view.character.experience+195);
    await act({keys:[],duration_ms:3000});const end=await game.inspect();
    game.check('defeated Leviathan stays absent without repeated XP',!enemy(end)&&end.view.character.experience===dead.view.character.experience);
    game.check('same prepared living Rogue retains supplied equipment',end.view.lifecycle.alive&&end.view.session.character===arrival.view.session.character&&JSON.stringify(end.view.character.items)===JSON.stringify(arrival.view.character.items));
    await writeFile(output('outcomes.json'),JSON.stringify({arrival,combat,dead,end}));
  }finally{await writeFile(output('combat-samples.json'),JSON.stringify({root,samples}));}
}
