import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const output=name=>join(process.env.QA_RESULTS_DIR,name);
let root;
export async function prepare(api){
  const spec={class:0x30e,level:20,seed:845,world:1,equipment:[0xb08,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,30,10,0,0]};
  const info=await api({op:'world_info',world:1}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'beholder-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(output('native/result.json'),'utf8'));
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+(await readFile(output('native/state/control.token'),'utf8')).trim()}});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const kinds=new Set(JSON.parse(execFileSync('python3',['-c',"import json,xml.etree.ElementTree as E; print(json.dumps([int(e.attrib['type'],0) for e in E.parse('data/objects.xml').getroot() if e.find('Enemy') is not None]))"],{encoding:'utf8'})));
  const candidates=map.entities.filter(e=>e.kind===0x656).map(e=>({entity:e,clearance:Math.min(...map.entities.filter(p=>p.id!==e.id&&kinds.has(p.kind)).map(p=>Math.hypot(p.pos.x-e.pos.x,p.pos.y-e.pos.y)))})).filter(({entity:e})=>{
    for(let dx=-7;dx<=7;dx++)for(let dy=-3;dy<=3;dy++)if(blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))return false;
    return true;
  }).sort((a,b)=>b.clearance-a.clearance);
  assert(candidates.length,'No naturally placed Beholder has a clear observation corridor');root=candidates[0].entity;
  await writeFile(output('natural-placement.json'),JSON.stringify({world:1,layout:info.realm_layout,root,candidates}));
  return {...spec,position:{x:root.pos.x-6,y:root.pos.y}};
}
export async function run(game){
  const samples=[],shots=new Map(),enemy=s=>s.diagnostic.entities.find(e=>e.id===root.id);
  const record=s=>{samples.push(s);for(const p of s.view.projectiles.filter(p=>p.owner===root.id))shots.set(p.id,p);};
  async function act(input,capture){
    let pending,error;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;const s=await game.inspect();record(s);return s;
  }
  try{
    const arrival=await game.inspect();
    game.check('natural mountain Beholder admitted at selected 1500 HP',enemy(arrival)?.hp===1500&&arrival.view.session.world===1);
    const path=output('protection.json');
    await writeFile(path,JSON.stringify({id:'beholder-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),output('native/native.json'),path]);
    await act({keys:[],duration_ms:2000},{name:'beholder-arrival',when:()=>true});
    await act({keys:[],duration_ms:6000},{name:'beholder-blind',when:()=>true});
    const combat=await game.inspect();
    game.check('natural Beholder activates and moves',samples.some(s=>enemy(s)?.activated!=null&&Math.hypot(enemy(s).pos.x-root.pos.x,enemy(s).pos.y-root.pos.y)>.2));
    game.check('native encounter inflicts Blind',samples.some(s=>(s.view.character.conditions&(1<<7))!==0));
    // Build 118 independently describes both attacks, not target-era engine fidelity: https://web.archive.org/web/20120426135122id_/http://forums.wildshadow.com/wiki/realm-mad-god/beholder
    game.check('native White Bolts and Purple Stars use selected speeds and lifetimes',[...shots.values()].some(p=>p.speed===50&&p.lifetime_ms===2700)&&[...shots.values()].some(p=>p.speed===70&&p.lifetime_ms===3000));
    game.check('native white volley has five shots at 72-degree spacing',samples.some(s=>{
      const groups=new Map();
      for(const p of s.view.projectiles.filter(p=>p.owner===root.id&&p.speed===50)){const key=p.start.x+':'+p.start.y+':'+p.start_time_ms;if(!groups.has(key))groups.set(key,[]);groups.get(key).push(p.angle);}
      return [...groups.values()].some(a=>a.length===5&&a.sort((a,b)=>a-b).slice(1).every((v,i)=>Math.abs(v-a[i]-2*Math.PI/5)<.001));
    }));
    for(let n=0;n<100&&enemy(await game.inspect());n++){
      const s=await game.inspect(),e=enemy(s),dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,approach=Math.hypot(dx,dy)>5;
      const keys=approach?[...(Math.abs(x)>Math.abs(y)*.5?[x>0?'d':'a']:[]),...(Math.abs(y)>Math.abs(x)*.5?[y>0?'s':'w']:[])]:[];
      await act({keys,duration_ms:200,target:{entity:root.id,camera_angle:-Math.PI/4,fire:!approach}});
    }
    await act({keys:[],duration_ms:500},{name:'beholder-defeated',when:()=>true});
    const dead=await game.inspect();
    game.check('ordinary native staff defeats natural Beholder and earns XP',!enemy(dead)&&dead.view.character.experience>=combat.view.character.experience+150);
    await act({keys:[],duration_ms:6500},{name:'beholder-recovery',when:()=>true});const end=await game.inspect();
    game.check('defeated Beholder stays absent without repeated XP',!enemy(end)&&end.view.character.experience===dead.view.character.experience);
    game.check('Blind expires after encounter',!(end.view.character.conditions&(1<<7)));
    game.check('same prepared living Wizard remains in the Realm',end.view.lifecycle.alive&&end.view.session.world===1&&end.view.session.character===arrival.view.session.character);
    await writeFile(output('outcomes.json'),JSON.stringify({arrival,combat,dead,end}));
  }finally{await writeFile(output('combat.json'),JSON.stringify({root,samples,shots:[...shots.values()]}));}
}
