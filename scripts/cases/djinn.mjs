import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const output=name=>join(process.env.QA_RESULTS_DIR,name);
let root;
export async function prepare(api){
  const spec={class:0x30e,level:20,seed:849,world:1,equipment:[0xa9a,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,30,10,0,0]};
  const report=JSON.parse(await readFile(output('native/result.json'),'utf8'));
  const info=await api({op:'world_info',world:1}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'djinn-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const headers={Authorization:'Bearer '+(await readFile(output('native/state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const kinds=new Set(JSON.parse(execFileSync('python3',['-c',"import json,xml.etree.ElementTree as E; print(json.dumps([int(e.attrib['type'],0) for e in E.parse('data/objects.xml').getroot() if e.find('Enemy') is not None]))"],{encoding:'utf8'})));
  const candidates=map.entities.filter(e=>e.kind===0x91a).map(e=>({entity:e,clearance:Math.min(...map.entities.filter(p=>p.id!==e.id&&kinds.has(p.kind)).map(p=>Math.hypot(p.pos.x-e.pos.x,p.pos.y-e.pos.y)))})).filter(({entity:e})=>{
    for(let dx=-9;dx<=3;dx++)for(let dy=-3;dy<=3;dy++)if(blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))return false;
    return true;
  }).sort((a,b)=>b.clearance-a.clearance);
  assert(candidates.length,'No naturally placed Djinn has a clear observation corridor');root=candidates[0].entity;
  await writeFile(output('natural-placement.json'),JSON.stringify({world:1,layout:info.realm_layout,root,clearance:candidates[0].clearance,candidates}));
  return {...spec,position:{x:root.pos.x-9,y:root.pos.y}};
}
export async function run(game){
  const samples=[],shots=new Map();
  const enemy=s=>s.diagnostic.entities.find(e=>e.id===root.id);
  async function act(input,capture){
    let pending,error;
    const record=s=>{samples.push(s);for(const p of s.view.projectiles.filter(p=>p.owner===root.id))shots.set(p.id,p);};
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},50);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;const s=await game.inspect();record(s);return s;
  }
  try{
    const arrival=await game.inspect();
    game.check('natural mountain Djinn admitted at selected 1000 HP',enemy(arrival)?.hp===1000&&arrival.view.session.world===1);
    const path=output('protection.json');
    await writeFile(path,JSON.stringify({id:'djinn-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),output('native/native.json'),path]);
    await act({keys:[],duration_ms:500},{name:'djinn-arrival',when:()=>true});
    game.check('Djinn does not fire before anyone enters its eight-tile activation radius',shots.size===0);
    await act({keys:['d','s'],duration_ms:650});
    await act({keys:[],duration_ms:5000},{name:'djinn-rings',when:()=>shots.size>=8});
    const combat=await game.inspect();
    game.check('natural Djinn activates and pursues',samples.some(s=>enemy(s)?.activated!==null&&enemy(s)&&Math.hypot(enemy(s).pos.x-root.pos.x,enemy(s).pos.y-root.pos.y)>.2));
    game.check('native White Bullets use selected speed six tiles/s and 1.5s lifetime',shots.size>=8&&[...shots.values()].every(p=>p.speed===60&&p.lifetime_ms===1500));
    game.check('native Djinn fires paired four-shot rings',samples.some(s=>s.view.projectiles.filter(p=>p.owner===root.id).length>=8));
    game.check('Djinn alternates shielded pursuit and vulnerable ring phase',[true,false].every(shielded=>samples.some(s=>enemy(s)?.activated!==null&&enemy(s)&&Boolean(enemy(s).condition&(1<<24))===shielded)));
    // Recovered mountains.xml:534ff, actors.xml:3200-3208: below half HP, flash invulnerably for one second, then Explode.
    const flashing=s=>s.diagnostic.xml[root.id]?.states.some(state=>state.name==='FlashBeforeExplode');
    for(let n=0;n<125;n++){
      const s=await game.inspect(),e=enemy(s);if(!e||flashing(s))break;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,approach=Math.hypot(dx,dy)>5||!s.view.objects.some(o=>o.id===e.id);
      const keys=approach?[...(Math.abs(x)>Math.abs(y)*.5?[x>0?'d':'a']:[]),...(Math.abs(y)>Math.abs(x)*.5?[y>0?'s':'w']:[])]:[];
      await act({keys,duration_ms:200,...(approach?{}:{target:{entity:root.id,camera_angle:-Math.PI/4,fire:true}})});
    }
    await act({keys:[],duration_ms:1500},{name:'djinn-defeated',when:()=>true});
    const dead=await game.inspect(),terminal=samples.filter(flashing);
    game.check('native staff damage arms the below-half-HP shielded terminal flash',terminal.length>1&&terminal.every(s=>enemy(s).hp>0&&enemy(s).hp<500&&Boolean(enemy(s).condition&(1<<24))));
    const log=await readFile(output('native/server.log'),'utf8');
    const facts=log.split('\n').filter(line=>line.startsWith('{')&&line.includes('game audit fact')).flatMap(line=>JSON.parse(JSON.parse(line).fields.record).data.facts??[]);
    const volley=terminal.length&&facts.find(f=>f.event==='volley_attempt'&&f.values.owner===root.id&&f.world_ms>=terminal.at(-1).diagnostic.time_ms&&f.values.shots.length===1&&f.values.spec.speed===60&&f.values.spec.lifetime===1500);
    const ack=volley&&facts.find(f=>f.event==='shoot_ack'&&f.identity?.[2]===arrival.diagnostic.player&&f.values.generations.some(([owner,id,born])=>owner===root.id&&id===volley.values.shots[0][0]&&born===volley.values.shots[0][1]));
    await writeFile(output('djinn-explode.json'),JSON.stringify({volley,ack}));
    game.check('terminal Djinn sends the selected single default shot and the native client acknowledges it',!!ack);
    game.check('ordinary native combat defeats natural Djinn and earns XP',!enemy(dead)&&dead.view.character.experience>=combat.view.character.experience+100);
    await act({keys:[],duration_ms:4000});const end=await game.inspect();
    game.check('defeated Djinn stays absent without repeated XP',!enemy(end)&&end.view.character.experience===dead.view.character.experience);
    game.check('same prepared living Wizard remains in the Realm',end.view.lifecycle.alive&&end.view.session.world===1&&end.view.session.character===arrival.view.session.character);
    await writeFile(output('outcomes.json'),JSON.stringify({arrival,combat,dead,end}));
  }finally{await writeFile(output('combat.json'),JSON.stringify({root,samples,shots:[...shots.values()]}));}
}
