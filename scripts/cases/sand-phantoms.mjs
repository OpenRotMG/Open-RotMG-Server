import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:841,world:1,equipment:[0xb08,0xb24,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,50,75,40,60]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
let phantom;
export async function prepare(api){
  const report=JSON.parse(await readFile(join(root(),'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root(),'state/control.token'),'utf8')).trim()};
  const info=await api({op:'world_info',world:spec.world}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'phantom-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-map.json'),JSON.stringify(map));
  phantom=map.entities.find(e=>e.kind===0x6c5&&[-1,0,1].every(dx=>[-1,0,1].every(dy=>!blocked.has([Math.floor(e.pos.x)-3+dx,Math.floor(e.pos.y)+dy].join(',')))));
  assert(phantom,'No natural Sand Phantom with a walkable approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-placement.json'),JSON.stringify({world:spec.world,layout:info.realm_layout,phantom}));
  return {...spec,position:{x:phantom.pos.x-3,y:phantom.pos.y}};
}
export async function run(game){
  const samples=[],shots=new Map();let serial=0,protectedUntil=0;
  // XML Metamorphose replaces the actor at its current position (actors.xml:2299,15382), so follow the lineage by place, not home.
  const lineage=new Set([phantom.id]);let current=phantom.id,last=phantom.pos;
  const family=s=>s.diagnostic.entities.filter(e=>lineage.has(e.id));
  const record=s=>{
    const e=s.diagnostic.entities.find(e=>e.id===current),next=!e&&s.diagnostic.entities.find(e=>[0x6c5,0x7f22].includes(e.kind)&&!lineage.has(e.id)&&Math.hypot(e.pos.x-last.x,e.pos.y-last.y)<2.5);
    if(e)last=e.pos;else if(next){current=next.id;lineage.add(next.id);last=next.pos;}
    samples.push(s);for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);return s;
  };
  const save=async name=>{const s=record(await game.inspect());await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  async function act(input,capture){
    const now=(await game.observe()).game_ms;
    if(now>=protectedUntil){
      const path=join(process.env.QA_RESULTS_DIR,'protection.json');
      await writeFile(path,JSON.stringify({id:'phantom-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
      execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root(),'native.json'),path]);protectedUntil=now+60000;
    }
    let pending=null,error;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},50);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;return record(await game.inspect());
  }
  try{
    const begin=await save('arrival');
    game.check('natural MidSand placement has one selected Phantom',family(begin).length===1&&family(begin)[0].hp===230);
    await act({keys:[],duration_ms:700},{name:'phantom-close',when:()=>true});
    const close=await save('close');
    await act({keys:[],duration_ms:9000},{name:'phantom-cycle',when:()=>true});
    const cycle=await save('cycle');
    // A birth can be sampled before its first tick sets activated.
    const wisps=samples.flatMap(s=>family(s).filter(e=>e.kind===0x7f22&&e.activated!==null).map(e=>({time:s.view.time_ms,...e})));
    game.check('close native player triggers Wisp after the XML two-second Metamorphose',wisps.length>0&&wisps[0].activated-begin.view.time_ms>=2000&&wisps[0].activated-begin.view.time_ms<=2500);
    game.check('selected Wisp HP and movement observed',wisps.some(e=>e.hp===150)&&wisps.some(e=>Math.hypot(e.pos.x-wisps[0].pos.x,e.pos.y-wisps[0].pos.y)>.2));
    const first=wisps[0];assert(first);
    game.check('Wisp retreats from nearby stationary player',wisps.some(e=>e.id===first.id&&e.time-first.time<800&&Math.hypot(e.pos.x-begin.view.position.x,e.pos.y-begin.view.position.y)>Math.hypot(first.pos.x-begin.view.position.x,first.pos.y-begin.view.position.y)+.2));
    const returned=samples.map(s=>family(s).find(e=>e.kind===0x6c5&&e.id!==phantom.id&&e.activated!==null)).find(Boolean);
    // move_away_from_player lasts 6.5s, then wisp_finished Metamorphoses after 0.2s (actors.xml:15379-15382).
    game.check('Wisp returns as a Phantom after XML 6.5s plus 0.2s Metamorphose',returned&&returned.activated-first.activated>=6700&&returned.activated-first.activated<=6900);
    game.check('transformations keep a single population member and award no kill XP',samples.every(s=>family(s).length===1)&&cycle.view.character.experience===begin.view.character.experience);
    for(let n=0;n<48;n++){
      const ids=new Set(samples.flatMap(family).filter(e=>e.kind===0x6c5).map(e=>e.id));
      if([...shots.values()].some(p=>ids.has(p.owner)))break;
      const s=record(await game.inspect()),e=family(s)[0];assert(e);
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,distance=Math.hypot(dx,dy),sign=distance<6?-1:1;
      const x=sign*(dx-dy)/Math.SQRT2,y=sign*(dx+dy)/Math.SQRT2;
      await act({keys:distance<6||distance>8?[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean):[],duration_ms:250});
    }
    for(const [kind,speed,lifetime] of [[0x6c5,70,1700],[0x7f22,65,2000]]){
      const ids=new Set(samples.flatMap(family).filter(e=>e.kind===kind).map(e=>e.id));
      game.check(kind+': selected projectile reaches native client',[...shots.values()].some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime));
    }
    const xp=cycle.view.character.experience;
    for(let n=0;n<40;n++){
      const s=record(await game.inspect()),e=family(s)[0];if(!e)break;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      if(Math.hypot(dx,dy)>6)await act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:150});
      else if(s.view.objects.some(o=>o.id===e.id&&o.hp>0))await act({keys:[],duration_ms:350,target:{entity:e.id,camera_angle:-Math.PI/4,fire:true}});
      await act({keys:[],duration_ms:250});
    }
    const dead=await save('defeated');
    game.check('ordinary staff defeats the natural encounter and awards XP',family(dead).length===0&&dead.view.character.experience>xp);
    await act({keys:[],duration_ms:2000},{name:'phantom-defeated',when:()=>true});
    const end=await save('end');
    game.check('defeat cannot transform or award XP twice',family(end).length===0&&end.view.character.experience===dead.view.character.experience);
    game.check('same prepared native Wizard remains alive',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
  }finally{await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples,shots:[...shots.values()]}));}
}
