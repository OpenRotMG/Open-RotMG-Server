import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const output=name=>join(process.env.QA_RESULTS_DIR,name);
let root;
export async function prepare(api){
  const spec={class:0x307,level:20,seed:847,world:1,equipment:[0xb02,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,252,75,0,30,50,0,0]};
  const report=JSON.parse(await readFile(output('native/result.json'),'utf8'));
  const info=await api({op:'world_info',world:1}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'slime-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const headers={Authorization:'Bearer '+(await readFile(output('native/state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const kinds=new Set(JSON.parse(execFileSync('python3',['-c',"import json,xml.etree.ElementTree as E; print(json.dumps([int(e.attrib['type'],0) for e in E.parse('data/objects.xml').getroot() if e.find('Enemy') is not None]))"],{encoding:'utf8'})));
  const candidates=map.entities.filter(e=>e.kind===0x658).map(e=>({entity:e,clearance:Math.min(...map.entities.filter(p=>p.id!==e.id&&kinds.has(p.kind)).map(p=>Math.hypot(p.pos.x-e.pos.x,p.pos.y-e.pos.y)))})).filter(({entity:e})=>{
    for(let dx=-9;dx<=9;dx++)for(let dy=-3;dy<=3;dy++)if(blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))return false;
    return true;
  }).sort((a,b)=>b.clearance-a.clearance);
  assert(candidates.length,'No naturally placed Slime God has a clear observation corridor');root=candidates[0].entity;
  await writeFile(output('natural-placement.json'),JSON.stringify({world:1,layout:info.realm_layout,root,clearance:candidates[0].clearance,candidates}));
  return {...spec,position:{x:root.pos.x-5,y:root.pos.y}};
}
export async function run(game){
  const samples=[];
  const enemy=s=>s.diagnostic.entities.find(e=>e.id===root.id);
  async function act(input,capture){
    let pending,error;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>samples.push(s)).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;const s=await game.inspect();samples.push(s);return s;
  }
  try{
    const arrival=await game.inspect();
    game.check('natural mountain Slime God admitted at selected 1000 HP',enemy(arrival)?.hp===1000&&arrival.view.session.world===1);
    const hurt=await act({keys:[],duration_ms:1000},{name:'slime-god-arrival',when:()=>true});
    game.check('unprotected native character takes encounter damage',hurt.view.character.hp<arrival.view.character.hp&&hurt.view.lifecycle.alive);
    const path=output('protection.json');
    await writeFile(path,JSON.stringify({id:'slime-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),output('native/native.json'),path]);
    await act({keys:[],duration_ms:8000},{name:'slime-god-attacks',when:()=>true});
    await act({keys:['d','s'],duration_ms:1000});
    await act({keys:['a','w'],duration_ms:1000});
    const combat=await game.inspect();
    game.check('natural Slime God activates and moves',samples.some(s=>{const e=enemy(s);return e&&e.activated!==null&&Math.hypot(e.pos.x-root.pos.x,e.pos.y-root.pos.y)>.2;}));
    const shots=samples.flatMap(s=>s.view.projectiles.filter(p=>p.owner===root.id));
    game.check('native Red Fire and Green Star retain selected speed and lifetime',shots.some(p=>p.speed===100&&p.lifetime_ms===1100)&&shots.some(p=>p.speed===70&&p.lifetime_ms===2000));
    // Build 118 corroborates both attacks and the six-second slow, not an exact prediction formula: https://web.archive.org/web/20120520004521id_/http://forums.wildshadow.com/wiki/realm-mad-god/slime-god
    game.check('native red volley has five shots at ten-degree gaps',samples.some(s=>{
      const fan=s.view.projectiles.filter(p=>p.owner===root.id&&p.speed===100);if(fan.length!==5)return false;
      const angles=fan.map(p=>p.angle).sort((a,b)=>a-b);return angles.slice(1).every((a,i)=>Math.abs(a-angles[i]-Math.PI/18)<.001);
    }));
    game.check('zero-damage Green Star applies Slowed through native hit reporting',samples.some(s=>s.view.character.conditions&(1<<3)));
    for(let n=0;n<40&&enemy(await game.inspect());n++){
      const s=await game.inspect(),e=enemy(s),dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,approach=Math.hypot(dx,dy)>5;
      const keys=approach?[...(Math.abs(x)>Math.abs(y)*.5?[x>0?'d':'a']:[]),...(Math.abs(y)>Math.abs(x)*.5?[y>0?'s':'w']:[])]:[];
      await act({keys,duration_ms:200,target:{entity:root.id,camera_angle:-Math.PI/4,fire:!approach}});
    }
    const dead=await act({keys:[],duration_ms:500},{name:'slime-god-defeated',when:()=>true});
    game.check('ordinary native bow fire defeats natural Slime God and earns XP',!enemy(dead)&&dead.view.character.experience>=combat.view.character.experience+100);
    const end=await act({keys:[],duration_ms:8000},{name:'slime-god-recovered',when:()=>true});
    game.check('Slowed clears after the defeated encounter',!(end.view.character.conditions&(1<<3)));
    game.check('defeated Slime God stays absent without repeated XP',!enemy(end)&&end.view.character.experience===dead.view.character.experience);
    game.check('same prepared living Archer remains in the Realm',end.view.lifecycle.alive&&end.view.session.world===1&&end.view.session.character===arrival.view.session.character);
    await writeFile(output('outcomes.json'),JSON.stringify({arrival,hurt,combat,dead,end}));
  }finally{await writeFile(output('combat.json'),JSON.stringify({root,samples}));}
}
