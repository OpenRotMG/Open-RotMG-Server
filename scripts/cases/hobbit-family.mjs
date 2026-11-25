import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:3,seed:810,world:1,equipment:[0xa9f,0xa2e,0xb05,0xac7],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
let mage;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'hobbit-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(process.env.QA_RESULTS_DIR,'native/result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(process.env.QA_RESULTS_DIR,'native/state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const clear=(x,y)=>!blocked.has([Math.floor(x),Math.floor(y)].join(','));
  mage=map.entities.find(e=>e.kind===0x617&&Array.from({length:12},(_,i)=>clear(e.pos.x-i,e.pos.y)).every(Boolean));
  assert(mage,'No natural Hobbit Mage with a clear approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-hobbits.json'),JSON.stringify(map.entities.filter(e=>e.id===mage.id||e.parent===mage.id)));
  return {...spec,position:{x:mage.pos.x-9.5,y:mage.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  // XML Hobbit Mage MakeMinions Archers and Rogues (actors.xml:606-607) are owned by parent, not by the natural camp.
  const members=s=>s.diagnostic.entities.filter(e=>(e.id===mage.id||e.parent===mage.id)&&[0x615,0x616].includes(e.kind));
  const initial=await save('arrival');
  game.check('ordinary Realm places the selected root without a legacy camp',initial.diagnostic.entities.some(e=>e.id===mage.id&&e.camp==null));
  const distance=Math.hypot(initial.view.position.x-mage.pos.x,initial.view.position.y-mage.pos.y);
  game.check('native arrival is between eight and twelve tiles from a natural Mage',distance>8&&distance<12);
  await game.act({keys:[],duration_ms:1600,target:{entity:mage.id,camera_angle:-Math.PI/4,fire:false}});
  await game.act({keys:[],duration_ms:1600},{name:'hobbit-activation',when:v=>v.game_ms>=2200});
  const active=await save('activation');
  game.check('Hobbit Mage activates at the documented twelve-tile gate',active.diagnostic.entities.some(e=>e.id===mage.id&&e.activated!==null));
  const path=join(process.env.QA_RESULTS_DIR,'protection.json');
  await writeFile(path,JSON.stringify({id:'hobbit-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);
  await game.act({keys:['d'],duration_ms:500});
  const shots=new Map(),samples=[];let pending=null,error;
  const record=s=>{samples.push(s);for(const p of s.view.projectiles)if(!shots.has(p.owner+':'+p.id))shots.set(p.owner+':'+p.id,p);};
  const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},65);
  try{await game.act({keys:[],duration_ms:4500});}finally{clearInterval(timer);if(pending)await pending;}
  if(error)throw error;
  await writeFile(join(process.env.QA_RESULTS_DIR,'attacks.json'),JSON.stringify({samples,shots:[...shots.values()]}));
  game.check('natural Mage moves after activation',samples.some(s=>s.diagnostic.entities.some(e=>e.id===mage.id&&Math.hypot(e.pos.x-mage.pos.x,e.pos.y-mage.pos.y)>.2)));
  const rings=[...shots.values()].filter(p=>p.owner===mage.id);
  for(const offset of [0,8,16])game.check('Mage ring offset '+offset+' reaches native client',rings.some(p=>Math.abs(((p.angle*180/Math.PI)%24+24)%24-offset)<.01));
  game.check('selected Mage rings retain speed fifty and lifetime 340ms',rings.length>0&&rings.every(p=>p.speed===50&&p.lifetime_ms===340));
  const before=await save('before-combat');
  for(const kind of [0x615,0x616]){
    game.check(kind+': XML-owned followers move',samples.some(s=>members(s).some(e=>e.kind===kind&&active.diagnostic.entities.some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.1))));
    const ids=new Set(members(active).filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': native client receives selected follower projectiles',[...shots.values()].some(p=>ids.has(p.owner)&&p.speed===60&&p.lifetime_ms===(kind===0x615?800:2000)));
  }
  async function defeat(id){
    const before=await game.observe();
    for(let n=0;n<70;n++){
      const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===id);if(!e)break;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=dx-dy,y=dx+dy;
      const keys=[Math.abs(x)>=Math.abs(y)*.4142?(x>0?'d':'a'):null,Math.abs(y)>=Math.abs(x)*.4142?(y>0?'s':'w'):null].filter(Boolean);
      await game.act(Math.hypot(dx,dy)>4||!s.view.objects.some(o=>o.id===id)?{keys,duration_ms:150}:{keys:[],duration_ms:250,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
    }
    const after=await game.inspect();game.check(id+': native shooting defeats target and awards XP',!after.diagnostic.entities.some(e=>e.id===id)&&after.view.character.experience>before.character.experience);
  }
  for(let n=0;n<2;n++)await game.act({keys:[],duration_ms:6000});
  const grown=await save('family-growth');
  game.check('reconstructed living population stays within four Archers and three Rogues',members(grown).filter(e=>e.kind===0x616).length<=4&&members(grown).filter(e=>e.kind===0x615).length<=3);
  game.check('living Mage fills its XML four Archers and three Rogues',[[0x616,4],[0x615,3]].every(([k,n])=>members(grown).filter(e=>e.parent===mage.id&&e.kind===k).length===n));
  await game.act({keys:[],duration_ms:700},{name:'hobbit-family',when:()=>true});
  await defeat(mage.id);
  const dead=await save('mage-defeated'),remaining=members(dead);
  game.check('Mage defeat leaves living dependent actors',remaining.length>0);
  for(let n=0;n<2;n++)await game.act({keys:[],duration_ms:6250});
  const end=await save('orphans');
  game.check('dead Mage cannot replace followers or transform',!end.diagnostic.entities.some(e=>e.id===mage.id)&&members(end).every(e=>remaining.some(p=>p.id===e.id)));
  // A projectile already in flight may earn XP for a different actor during this observation.
  const events=(await readFile(join(process.env.QA_RESULTS_DIR,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
  game.check('the XP-granting Mage defeat is published once',events.filter(e=>e.message==='XML terminal effects published from exact durable journal receipt'&&e.actor===mage.id).length===1);
  for(const kind of [0x615,0x616]){const victim=members(await game.inspect()).find(e=>e.kind===kind);assert(victim,'Missing surviving follower '+kind);await defeat(victim.id);}
  const final=await save('finished');
  game.check('same supplied living character completes native encounter',final.view.lifecycle.alive&&final.view.session.character===before.view.session.character);
  await game.act({keys:[],duration_ms:700},{name:'hobbit-defeated',when:()=>true});
}
