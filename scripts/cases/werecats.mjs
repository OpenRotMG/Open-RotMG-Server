import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:837,world:1,equipment:[0xa9f,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
let lion;
export async function prepare(api){
  const info=await api({op:'world_info',world:spec.world}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'werecat-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root(),'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root(),'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-map.json'),JSON.stringify(map));
  lion=map.entities.find(e=>e.kind===0x637&&[-2,-1,0,1,2].every(dx=>[-2,-1,0,1,2].every(dy=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))));
  assert(lion,'Sampled natural Realm lacks a Werecat camp with a clear approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-camp.json'),JSON.stringify({world:spec.world,layout:info.realm_layout,members:map.entities.filter(e=>e.id===lion.id||e.parent===lion.id)}));
  return {...spec,position:{x:lion.pos.x-2,y:lion.pos.y}};
}
export async function run(game){
  const samples=[],shots=new Map();
  // XML MakeMinions children (actors.xml:1109-1111) are owned by parent, not by the natural camp.
  const family=s=>s.diagnostic.entities.filter(e=>e.id===lion.id||e.parent===lion.id);
  const states=(s,id)=>(s.diagnostic.xml[id]?.states??[]).map(x=>x.name),owned=(s,kind)=>family(s).filter(e=>e.parent===lion.id&&e.kind===kind);
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  let sampling=true;
  const record=s=>{samples.push(s);for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);};
  let shielded=null,serial=0;
  async function act(input,capture){
    let now=(await game.observe()).game_ms;
    // Protection cannot be renewed while active, and loaded acts can overrun: idle out the window, then renew.
    while(shielded!==null&&now<shielded&&shielded-now<input.duration_ms+5000){await game.act({keys:[],duration_ms:Math.min(10000,Math.max(50,shielded-now))});now=(await game.observe()).game_ms;}
    if(shielded!==null&&now>=shielded){
      const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
      await writeFile(path,JSON.stringify({id:'werecat-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
      execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root(),'native.json'),path]);shielded=now+60000;
    }
    let pending,error;
    const timer=setInterval(()=>{if(sampling&&!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},25);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;const s=await game.inspect();record(s);return s;
  }
  try{
    const begin=await save('arrival');
    game.check('ordinary Realm places the selected 400 HP Werelion before offspring activation',family(begin).length===1&&family(begin).some(e=>e.id===lion.id&&e.kind===0x637&&e.hp===400));
    await act({keys:[],duration_ms:1500},{name:'werecats-arrival',when:()=>true});
    const initial=family(await save('activated'));
    for(const [kind,min,max,hp] of [[0x637,1,1,400],[0x635,1,1,220],[0x636,1,2,220],[0x639,1,3,240]]){
      const members=initial.filter(e=>e.kind===kind);
      game.check(kind+': XML population (actors.xml:1109-1111) and selected HP',members.length>=min&&members.length<=max&&members.every(e=>e.hp===hp));
    }
    game.check('native enemy fire damages the exposed Wizard',(await game.observe()).character.hp<begin.view.character.hp);
    shielded=0;
    for(let n=0;n<24;n++)await act({keys:[],duration_ms:250},n===16?{name:'werecats-attacks',when:()=>true}:undefined);
    for(const [kind,speed,lifetime] of [[0x635,65,2200],[0x636,80,1000],[0x639,70,2000],[0x637,55,2700]]){
      const ids=new Set(initial.filter(e=>e.kind===kind).map(e=>e.id));
      game.check(kind+': activation and movement observed',samples.some(s=>family(s).some(e=>ids.has(e.id)&&e.activated!==null&&initial.some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.1))));
      game.check(kind+': selected projectile observed',[...shots.values()].some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime));
    }
    // A close player sends if_cloaked straight back to normal_attack, re-arming its cooldown-90 Grey Missile volley (actors.xml:1118-1125).
    game.check('close Werelion cycles normal_attack and if_cloaked, refiring Grey Missiles',['normal_attack','if_cloaked'].every(n=>samples.some(s=>states(s,lion.id).includes(n)))&&[...shots.values()].filter(p=>p.owner===lion.id&&p.speed===55).length>=6);
    game.check('Panther jumps into one tile of the player and attacks (actors.xml:1156-1167)',samples.some(s=>family(s).some(e=>e.kind===0x639&&states(s,e.id).includes('jump')))&&samples.some(s=>family(s).some(e=>e.kind===0x639&&states(s,e.id).includes('attack')&&Math.hypot(e.pos.x-s.view.position.x,e.pos.y-s.view.position.y)<1.5)));
    sampling=false;
    await act({keys:[],duration_ms:10000});
    const filled=await save('filled'),tigers=owned(filled,0x635);
    game.check('living Werelion keeps its single XML-owned Tiger',tigers.length===1);
    const victim=tigers[0];assert(victim);
    for(let n=0;n<80;n++){
      const s=await game.inspect(),cat=family(s).find(e=>e.id===victim.id);if(!cat)break;
      const owner=family(s).find(e=>e.id===lion.id);assert(owner,'Owner died before follower check');
      // XML Tigers Protect the Werelion (actors.xml:1090), so fire only when the owner is clear of the shot line.
      const p=s.view.position,dx=cat.pos.x-p.x,dy=cat.pos.y-p.y,t=Math.max(0,((owner.pos.x-p.x)*dx+(owner.pos.y-p.y)*dy)/(dx*dx+dy*dy)),clear=t>1.2||Math.hypot(owner.pos.x-p.x-t*dx,owner.pos.y-p.y-t*dy)>1.5;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      // Close in so the escorts swarming the player cannot absorb every shot.
      if(Math.hypot(dx,dy)>2.5)await act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:150});
      else await act(clear?{keys:[],duration_ms:300,target:{entity:cat.id,camera_angle:-Math.PI/4,fire:true}}:{keys:[],duration_ms:100});
    }
    game.check('native staff defeats a Tiger while owner lives',!family(await game.inspect()).some(e=>e.id===victim.id)&&family(await game.inspect()).some(e=>e.id===lion.id));
    // MakeMinions Weretiger cooldown is 23 s (actors.xml:1109).
    for(let n=0;n<26&&!owned(await game.inspect(),0x635).length;n++)await act({keys:[],duration_ms:1000});
    const refill=await save('reinforced');
    game.check('living owner replaces its vacated Tiger',owned(refill,0x635).length===1&&owned(refill,0x635)[0].id!==victim.id);
    // Temporary attack fixture (cube-god precedent): the Werelion's protecting XML children absorb most shots.
    const attack=join(process.env.QA_RESULTS_DIR,'attack.json');
    await writeFile(attack,JSON.stringify({id:'werecat-attack',command:{op:'cheat',actor:game.actor,change:{kind:'stat_modifier',index:2,amount:100,duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root(),'native.json'),attack]);
    // Walk in so the Werelion's protecting escorts cannot absorb every shot.
    for(let n=0;n<80;n++){
      const s=await game.inspect(),e=family(s).find(e=>e.id===lion.id);if(!e)break;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      await act(Math.hypot(dx,dy)>1.5?{keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:150}:{keys:[],duration_ms:200,target:{entity:lion.id,camera_angle:-Math.PI/4,fire:true}});
    }
    const orphaned=await save('owner-defeated');
    game.check('ordinary native staff defeats owner',!family(orphaned).some(e=>e.id===lion.id));
    const survivors=new Set(family(orphaned).map(e=>e.id));
    for(let n=0;n<200;n++){
      const s=await game.inspect(),p=s.view.position,victim=family(s).sort((a,b)=>Math.hypot(a.pos.x-p.x,a.pos.y-p.y)-Math.hypot(b.pos.x-p.x,b.pos.y-p.y))[0];if(!victim)break;
      const dx=victim.pos.x-p.x,dy=victim.pos.y-p.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      await act(Math.hypot(dx,dy)>4?{keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:150}:{keys:[],duration_ms:200,target:{entity:victim.id,camera_angle:-Math.PI/4,fire:true}});
    }
    const cleared=await save('family-defeated');
    game.check('native staff defeats all surviving owned cats',family(cleared).length===0&&survivors.size>0);
    game.check('combat awards XP',cleared.view.character.experience>begin.view.character.experience);
    await act({keys:[],duration_ms:10000},{name:'werecats-defeated',when:()=>true});
    const end=await save('final');
    game.check('dead owner cannot replenish its camp',family(end).length===0);
    game.check('defeat awards no duplicate XP',end.view.character.experience===cleared.view.character.experience);
    game.check('same prepared living Wizard completes the encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
  }finally{await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples,shots:[...shots.values()]}));}
}
