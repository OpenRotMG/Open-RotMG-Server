import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:4,seed:811,world:1,equipment:[0xa9f,0xa2e,0xb05,0xac7],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
let mage;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'undead-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(process.env.QA_RESULTS_DIR,'native/result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(process.env.QA_RESULTS_DIR,'native/state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  mage=map.entities.find(e=>e.kind===0x61a&&Array.from({length:13},(_,i)=>!blocked.has([Math.floor(e.pos.x-i),Math.floor(e.pos.y)].join(','))).every(Boolean));
  assert(mage,'No natural Undead Hobbit Mage with a clear approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-family.json'),JSON.stringify(map.entities.filter(e=>e.id===mage.id||e.parent===mage.id)));
  return {...spec,position:{x:mage.pos.x-11.5,y:mage.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  // XML Undead Hobbit Mage MakeMinions Archers and Rogues (actors.xml:668-669) are owned by parent, not by the natural camp.
  const family=s=>s.diagnostic.entities.filter(e=>e.id===mage.id||e.parent===mage.id);
  const initial=await save('arrival');
  game.check('ordinary LowPlains places the selected 270 HP Mage before offspring activation',family(initial).length===1&&family(initial).some(e=>e.id===mage.id&&e.kind===0x61a&&e.hp===270));
  const distance=Math.hypot(initial.view.position.x-mage.pos.x,initial.view.position.y-mage.pos.y);
  game.check('native arrival is outside old 10.5 gate and inside twelve tiles',distance>10.5&&distance<12);
  await game.act({keys:[],duration_ms:1600});
  await game.act({keys:[],duration_ms:1600},{name:'undead-activation',when:v=>v.game_ms>=2200});
  const active=await save('activation');
  game.check('natural Undead Mage activates inside twelve tiles',family(active).some(e=>e.id===mage.id&&e.activated!==null));
  const path=join(process.env.QA_RESULTS_DIR,'protection.json');
  await writeFile(path,JSON.stringify({id:'undead-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);
  // The Mage's Salmon Missile is a range-10 targeted Shoot (actors.xml:649) and it wanders, so close in before sampling.
  for(let n=0;n<30;n++){
    const s=await game.inspect(),m=family(s).find(e=>e.id===mage.id),dx=m.pos.x-s.view.position.x,dy=m.pos.y-s.view.position.y;if(Math.hypot(dx,dy)<6)break;
    const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
    await game.act({keys,duration_ms:200});
  }
  const shots=new Map(),samples=[];let pending=null,error;
  const record=s=>{samples.push(s);for(const p of s.view.projectiles)if(!shots.has(p.owner+':'+p.id))shots.set(p.owner+':'+p.id,p);};
  const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},65);
  try{await game.act({keys:[],duration_ms:5000});}finally{clearInterval(timer);if(pending)await pending;}
  if(error)throw error;
  await writeFile(join(process.env.QA_RESULTS_DIR,'attacks.json'),JSON.stringify({samples,shots:[...shots.values()]}));
  game.check('natural Mage moves after activation',samples.some(s=>family(s).some(e=>e.id===mage.id&&Math.hypot(e.pos.x-mage.pos.x,e.pos.y-mage.pos.y)>.2)));
  const emitted=[...shots.values()].filter(p=>p.owner===mage.id),rings=emitted.filter(p=>p.speed===50);
  for(const offset of [0,8,16])game.check('native ring offset '+offset,rings.some(p=>Math.abs(Math.sin((p.angle*180/Math.PI-offset)*Math.PI/24))<.001));
  game.check('native rings retain selected speed/lifetime',rings.length>0&&rings.every(p=>p.lifetime_ms===340));
  game.check('native client receives concurrent Salmon Missiles',emitted.some(p=>p.speed===60&&p.lifetime_ms===1100));
  for(const [kind,lifetime] of [[0x618,800],[0x619,2000]]){
    const ids=new Set(family(active).filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': XML-owned followers have selected HP and move',family(active).filter(e=>e.kind===kind).every(e=>e.hp===(kind===0x619?28:34))&&samples.some(s=>family(s).some(e=>ids.has(e.id)&&Math.hypot(e.pos.x-active.diagnostic.entities.find(p=>p.id===e.id).pos.x,e.pos.y-active.diagnostic.entities.find(p=>p.id===e.id).pos.y)>.1)));
    game.check(kind+': native follower projectiles',[...shots.values()].some(p=>ids.has(p.owner)&&p.speed===60&&p.lifetime_ms===lifetime));
  }
  for(let n=0;n<2;n++)await game.act({keys:[],duration_ms:6000});
  const grown=await save('growth');
  game.check('growth preserves undead identities and current living caps',family(grown).every(e=>[0x618,0x619,0x61a].includes(e.kind))&&family(grown).filter(e=>e.kind===0x619).length<=4&&family(grown).filter(e=>e.kind===0x618).length<=3);
  game.check('living Mage fills its XML four Archers and three Rogues',[[0x619,4],[0x618,3]].every(([k,n])=>family(grown).filter(e=>e.parent===mage.id&&e.kind===k).length===n));
  await game.act({keys:[],duration_ms:700},{name:'undead-family',when:()=>true});
  async function defeat(id){
    const before=await game.observe();
    // XML Mages Follow at range 6 and escorts wander (actors.xml:645-669), so close in before firing.
    for(let n=0;n<60;n++){
      const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===id);if(!e)break;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      const observed=s.view.objects.find(o=>o.id===id&&o.hp>0);
      if(Math.hypot(dx,dy)>5||!observed){
        await game.act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:200});
      }else{
        // A projectile already in flight can finish a moving follower while the next action resumes.
        // Aim on its last observed playfield position without requiring it to survive the resume.
        const vx=observed.position.x-s.view.position.x,vy=observed.position.y-s.view.position.y;
        await game.act({keys:[],duration_ms:250,mouse:{x:300+(vx-vy)*50/Math.SQRT2,y:325+(vx+vy)*50/Math.SQRT2}});
      }
    }
    const after=await game.inspect();game.check(id+': native defeat awards XP',!after.diagnostic.entities.some(e=>e.id===id)&&after.view.character.experience>before.character.experience);
  }
  await defeat(mage.id);
  const dead=await save('mage-defeated'),remaining=family(dead);
  game.check('dependent actors survive Mage defeat',remaining.length>0);
  for(let n=0;n<2;n++)await game.act({keys:[],duration_ms:6250});
  const orphans=await save('orphans');
  game.check('dead Mage cannot reproduce or transform',family(orphans).every(e=>e.kind!==0x61a&&remaining.some(p=>p.id===e.id)));
  game.check('XP is not awarded twice',orphans.view.character.experience===dead.view.character.experience);
  // XML Rogues Protect and Follow the Mage at range 1 (actors.xml:619-620), so Mage fire can already have defeated them.
  for(const kind of [0x618,0x619]){const victim=family(await game.inspect()).find(e=>e.kind===kind);if(victim)await defeat(victim.id);else game.check(kind+': native collateral already defeated the Mage-owned role',remaining.every(e=>e.kind!==kind)&&dead.view.character.experience>active.view.character.experience);}
  const final=await save('finished');
  game.check('same supplied living character completes encounter',final.view.lifecycle.alive&&final.view.session.character===initial.view.session.character);
  await game.act({keys:[],duration_ms:700},{name:'undead-defeated',when:()=>true});
}
