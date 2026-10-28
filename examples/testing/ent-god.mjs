import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const output=name=>join(process.env.QA_RESULTS_DIR,name);
let root;
export async function prepare(api){
  const spec={class:0x30e,level:20,seed:844,world:1,equipment:[0xb08,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,30,10,0,0]};
  const report=JSON.parse(await readFile(output('native/result.json'),'utf8'));
  const info=await api({op:'world_info',world:1}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'ent-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const headers={Authorization:'Bearer '+(await readFile(output('native/state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const kinds=new Set(JSON.parse(execFileSync('python3',['-c',"import json,xml.etree.ElementTree as E; print(json.dumps([int(e.attrib['type'],0) for e in E.parse('data/objects.xml').getroot() if e.find('Enemy') is not None]))"],{encoding:'utf8'})));
  const candidates=map.entities.filter(e=>e.kind===0x655).map(e=>({entity:e,clearance:Math.min(...map.entities.filter(p=>p.id!==e.id&&kinds.has(p.kind)).map(p=>Math.hypot(p.pos.x-e.pos.x,p.pos.y-e.pos.y)))})).filter(({entity:e})=>{
    for(let dx=-9;dx<=9;dx++)for(let dy=-3;dy<=3;dy++)if(blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))return false;
    return true;
  }).sort((a,b)=>b.clearance-a.clearance);
  assert(candidates.length,'No naturally placed Ent God has a clear observation corridor');root=candidates[0].entity;
  await writeFile(output('natural-placement.json'),JSON.stringify({world:1,layout:info.realm_layout,root,clearance:candidates[0].clearance,candidates}));
  return {...spec,position:{x:root.pos.x-9,y:root.pos.y}};
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
    game.check('natural mountain Ent God admitted at selected 1000 HP',enemy(arrival)?.hp===1000&&arrival.view.session.world===1);
    const path=output('protection.json');
    await writeFile(path,JSON.stringify({id:'ent-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),output('native/native.json'),path]);
    await act({keys:[],duration_ms:3000},{name:'ent-god-arrival',when:()=>true});
    await act({keys:['d','w'],duration_ms:650});
    await act({keys:[],duration_ms:5000},{name:'ent-god-fan',when:()=>true});
    await act({keys:['a','s'],duration_ms:500});
    await act({keys:['d','s'],duration_ms:1500});
    await act({keys:['a','w'],duration_ms:1500});
    const combat=await game.inspect();
    game.check('natural Ent God activates and moves',samples.some(s=>enemy(s)?.activated!==null&&enemy(s)&&Math.hypot(enemy(s).pos.x-root.pos.x,enemy(s).pos.y-root.pos.y)>.2));
    game.check('native Fire Bolts use selected speed 8 tiles/s and 2s lifetime',[...shots.values()].length>=10&&[...shots.values()].every(p=>p.speed===80&&p.lifetime_ms===2000));
    // Build 118 independently lists the fan, not the exact prediction formula: https://web.archive.org/web/20120520004521id_/http://forums.wildshadow.com/wiki/realm-mad-god/ent-god
    game.check('native volley contains five shots with ten-degree gaps',samples.some(s=>{
      const fan=s.view.projectiles.filter(p=>p.owner===root.id);if(fan.length!==5)return false;
      const angles=fan.map(p=>p.angle).sort((a,b)=>a-b);return angles.slice(1).every((a,i)=>Math.abs(a-angles[i]-Math.PI/18)<.001);
    }));
    for(let n=0;n<100&&enemy(await game.inspect());n++){
      const s=await game.inspect(),e=enemy(s),dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,approach=Math.hypot(dx,dy)>5;
      const keys=approach?[...(Math.abs(x)>Math.abs(y)*.5?[x>0?'d':'a']:[]),...(Math.abs(y)>Math.abs(x)*.5?[y>0?'s':'w']:[])]:[];
      await act({keys,duration_ms:200,target:{entity:root.id,camera_angle:-Math.PI/4,fire:!approach}});
    }
    await act({keys:[],duration_ms:500},{name:'ent-god-defeated',when:()=>true});
    const dead=await game.inspect();
    game.check('ordinary native staff fire defeats natural Ent God and earns XP',!enemy(dead)&&dead.view.character.experience>=combat.view.character.experience+100);
    await act({keys:[],duration_ms:4000});const end=await game.inspect();
    game.check('defeated Ent God stays absent without repeated XP',!enemy(end)&&end.view.character.experience===dead.view.character.experience);
    game.check('same prepared living Wizard remains in the Realm',end.view.lifecycle.alive&&end.view.session.world===1&&end.view.session.character===arrival.view.session.character);
    await writeFile(output('outcomes.json'),JSON.stringify({arrival,combat,dead,end}));
  }finally{await writeFile(output('combat.json'),JSON.stringify({root,samples,shots:[...shots.values()]}));}
}
