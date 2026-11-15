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
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-family.json'),JSON.stringify(map.entities.filter(e=>e.camp===mage.camp)));
  return {...spec,position:{x:mage.pos.x-11.5,y:mage.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===mage.camp);
  const initial=await save('arrival');
  game.check('natural LowPlains family has selected HP and undead identities',[[0x61a,1,270],[0x619,2,28],[0x618,2,34]].every(([k,n,hp])=>family(initial).filter(e=>e.kind===k&&e.hp===hp).length===n));
  const distance=Math.hypot(initial.view.position.x-mage.pos.x,initial.view.position.y-mage.pos.y);
  game.check('native arrival is outside old 10.5 gate and inside twelve tiles',distance>10.5&&distance<12);
  await game.act({keys:[],duration_ms:1600,target:{entity:mage.id,camera_angle:-Math.PI/4,fire:false}});
  await game.act({keys:[],duration_ms:1600},{name:'undead-activation',when:v=>v.game_ms>=2200});
  const active=await save('activation');
  game.check('natural Undead Mage activates inside twelve tiles',family(active).some(e=>e.id===mage.id&&e.activated!==null));
  const path=join(process.env.QA_RESULTS_DIR,'protection.json');
  await writeFile(path,JSON.stringify({id:'undead-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);
  await game.act({keys:['d'],duration_ms:1000});
  const shots=new Map(),samples=[];let pending=null,error;
  const record=s=>{samples.push(s);for(const p of s.view.projectiles)if(!shots.has(p.owner+':'+p.id))shots.set(p.owner+':'+p.id,p);};
  const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},65);
  try{await game.act({keys:[],duration_ms:5000});}finally{clearInterval(timer);if(pending)await pending;}
  if(error)throw error;
  await writeFile(join(process.env.QA_RESULTS_DIR,'attacks.json'),JSON.stringify({samples,shots:[...shots.values()]}));
  game.check('natural Mage moves after activation',samples.some(s=>family(s).some(e=>e.id===mage.id&&Math.hypot(e.pos.x-mage.pos.x,e.pos.y-mage.pos.y)>.2)));
  const emitted=[...shots.values()].filter(p=>p.owner===mage.id),rings=emitted.filter(p=>p.speed===50);
  for(const offset of [0,8,16])game.check('native ring offset '+offset,rings.some(p=>Math.abs(((p.angle*180/Math.PI)%24+24)%24-offset)<.01));
  game.check('native rings retain selected speed/lifetime',rings.length>0&&rings.every(p=>p.lifetime_ms===340));
  game.check('native client receives concurrent Salmon Missiles',emitted.some(p=>p.speed===60&&p.lifetime_ms===1100));
  for(const [kind,lifetime] of [[0x618,800],[0x619,2000]]){
    const ids=new Set(family(initial).filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': natural followers move',samples.some(s=>family(s).some(e=>ids.has(e.id)&&Math.hypot(e.pos.x-initial.diagnostic.entities.find(p=>p.id===e.id).pos.x,e.pos.y-initial.diagnostic.entities.find(p=>p.id===e.id).pos.y)>.1)));
    game.check(kind+': native follower projectiles',[...shots.values()].some(p=>ids.has(p.owner)&&p.speed===60&&p.lifetime_ms===lifetime));
  }
  for(let n=0;n<2;n++)await game.act({keys:[],duration_ms:6000});
  const grown=await save('growth');
  game.check('growth preserves undead identities and current living caps',family(grown).every(e=>[0x618,0x619,0x61a].includes(e.kind))&&family(grown).filter(e=>e.kind===0x619).length<=4&&family(grown).filter(e=>e.kind===0x618).length<=3);
  for(const kind of [0x618,0x619])game.check(kind+': natural family grows',family(grown).some(e=>e.kind===kind&&!family(initial).some(p=>p.id===e.id)));
  await game.act({keys:[],duration_ms:700},{name:'undead-family',when:()=>true});
  async function defeat(id){
    const before=await game.observe();
    for(let n=0;n<32&&(await game.inspect()).diagnostic.entities.some(e=>e.id===id);n++)await game.act({keys:[],duration_ms:250,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
    const after=await game.inspect();game.check(id+': native defeat awards XP',!after.diagnostic.entities.some(e=>e.id===id)&&after.view.character.experience>before.character.experience);
  }
  await defeat(mage.id);
  const dead=await save('mage-defeated'),remaining=family(dead);
  game.check('dependent actors survive Mage defeat',remaining.length>0);
  for(let n=0;n<2;n++)await game.act({keys:[],duration_ms:6250});
  const orphans=await save('orphans');
  game.check('dead Mage cannot reproduce or transform',family(orphans).every(e=>e.kind!==0x61a&&remaining.some(p=>p.id===e.id)));
  game.check('XP is not awarded twice',orphans.view.character.experience===dead.view.character.experience);
  for(const kind of [0x618,0x619]){const victim=family(await game.inspect()).find(e=>e.kind===kind);assert(victim,'Missing surviving undead follower '+kind);await defeat(victim.id);}
  const final=await save('finished');
  game.check('same supplied living character completes encounter',final.view.lifecycle.alive&&final.view.session.character===initial.view.session.character);
  await game.act({keys:[],duration_ms:700},{name:'undead-defeated',when:()=>true});
}
