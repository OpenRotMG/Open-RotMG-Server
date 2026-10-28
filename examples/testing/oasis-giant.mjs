import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const out=process.env.QA_RESULTS_DIR;
export async function prepare(api){
  const events=(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
  const ends=events.flatMap((e,i)=>e.message==='Realm simulation populated'?[i]:[]);assert(ends.length===2,'Two initial Realm populations required');
  for(const [index,id] of [-3,1].entries()){
    const roots=events.slice(index===0?0:ends[index-1]+1,ends[index]).filter(e=>e.message==='finite named Realm quest placed'&&e.kind===0x678);
    if(!roots.length)continue;
    const world=await api({op:'world_info',world:id}),root=roots[0];assert(world.realm_layout===root.layout,'Placement log must match the actual world');
    await writeFile(join(out,'placement.json'),JSON.stringify({world,root}));
    return {class:0x307,level:20,seed:797,world:id,position:{x:root.x+6,y:root.y},equipment:[0xc02,-1,0xaf9,-1],backpack:Array(8).fill(-1),stats:[700,252,75,25,50,50,40,50]};
  }
  throw Error('Neither initial Realm contains an ordinarily placed Oasis');
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const save=async name=>{const s=await game.inspect();await writeFile(join(out,name+'.json'),JSON.stringify(s));return s;};
  let protectedUntil=0;
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));protectedUntil=now+60000;
  }
  async function idle(ms){while(ms>0){await protect();const step=Math.min(ms,5000,protectedUntil-(await game.observe()).game_ms);await game.act({keys:[],duration_ms:step});ms-=step;}}
  const initial=await save('initial'),boss=initial.diagnostic.entities.find(e=>e.kind===0x678);assert(boss);
  const children=s=>s.diagnostic.entities.filter(e=>e.quest===boss.quest&&e.kind>=0x679&&e.kind<=0x67c);
  const chest=initial.diagnostic.entities.find(e=>e.kind===0x501&&Math.hypot(e.pos.x-boss.pos.x,e.pos.y-boss.pos.y)<1);assert(chest,'Natural Oasis chest required');
  await idle(3000);await game.act({keys:[],duration_ms:1000},{name:'oasis-entry',when:()=>true});
  const trace=[];let pending=null,sampleError;
  const sample=async()=>{const s=await game.inspect();trace.push({view:s.view,children:children(s),boss:s.diagnostic.entities.find(e=>e.id===boss.id)});};
  const timer=setInterval(()=>{if(!pending)pending=sample().catch(e=>{sampleError=e;}).finally(()=>{pending=null;});},100);
  try{await game.act({keys:[],duration_ms:5000},{name:'oasis-volley',when:()=>trace.some(t=>t.view.projectiles.some(p=>p.owner===boss.id))});}
  finally{clearInterval(timer);if(pending)await pending;}
  if(sampleError)throw sampleError;
  await writeFile(join(out,'attacks.json'),JSON.stringify(trace));
  const errors=[],gaps=[];
  for(const t of trace){
    const groups=Map.groupBy(t.view.projectiles.filter(p=>p.owner===boss.id),p=>JSON.stringify(p.start));
    for(const shots of groups.values())if(shots.length===4){
      const aim=Math.atan2(t.view.position.y-shots[0].start.y,t.view.position.x-shots[0].start.x),angles=shots.map(p=>Math.atan2(Math.sin(p.angle-aim),Math.cos(p.angle-aim))).sort((a,b)=>a-b);
      errors.push(angles.reduce((a,b)=>a+b)/4);
      gaps.push(...angles.slice(1).map((a,i)=>a-angles[i]));
    }
  }
  assert(errors.length,'At least one complete native Giant volley required');
  game.check('Giant volley has four bolts spaced seven degrees',gaps.every(g=>Math.abs(g-7*Math.PI/180)<.001));
  game.check('Giant aims at a stationary native player instead of a fixed angular offset',errors.every(e=>Math.abs(e)<.03));
  game.check('Giant wanders around its natural island',trace.some(t=>t.boss&&Math.hypot(t.boss.pos.x-boss.pos.x,t.boss.pos.y-boss.pos.y)>.1));
  await idle(50000);
  let state=await save('population');
  game.check('all four ordinary Oasis species participate',[0x679,0x67a,0x67b,0x67c].every(k=>children(state).some(e=>e.kind===k)));
  game.check('Giant reaches its twenty-birth lifetime cap',children(state).length===20);
  const population=children(state),attackers=new Set(trace.flatMap(t=>t.view.projectiles.map(p=>p.owner)));
  for(let n=0;n<5;n++){await idle(1000);state=await save('family-'+n);for(const p of state.view.projectiles)attackers.add(p.owner);}
  game.check('all four species fire native-visible projectiles',[0x679,0x67a,0x67b,0x67c].every(k=>population.some(e=>e.kind===k&&attackers.has(e.id))));
  game.check('all four species move during the encounter',[0x679,0x67a,0x67b,0x67c].every(k=>population.some(e=>e.kind===k&&children(state).some(a=>a.id===e.id&&Math.hypot(a.pos.x-e.pos.x,a.pos.y-e.pos.y)>.1))));
  async function fire(id){await protect();await game.act({keys:[],duration_ms:300,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});}
  const heading=e=>Math.atan2(e.pos.y-state.view.position.y,e.pos.x-state.view.position.x),bossAngle=heading(state.diagnostic.entities.find(e=>e.id===boss.id));
  const victim=children(state).find(e=>state.view.objects.some(o=>o.id===e.id)&&Math.abs(Math.atan2(Math.sin(heading(e)-bossAngle),Math.cos(heading(e)-bossAngle)))>.4);assert(victim,'Visible follower outside the boss firing line required');
  for(let n=0;n<30&&(await game.inspect()).diagnostic.entities.some(e=>e.id===victim.id);n++)await fire(victim.id);
  state=await save('follower-defeated');
  game.check('native bow fire defeats an owned follower',!state.diagnostic.entities.some(e=>e.id===victim.id));
  const survivors=children(state).map(e=>e.id);await idle(10000);state=await save('no-replenishment');
  game.check('defeated followers do not replenish beyond the lifetime birth cap',JSON.stringify(children(state).map(e=>e.id))===JSON.stringify(survivors));
  const xp=state.view.character.experience;
  for(let n=0;n<30&&(await game.inspect()).diagnostic.entities.some(e=>e.id===boss.id);n++)await fire(boss.id);
  await idle(500);state=await save('giant-defeated');
  game.check('native bow fire defeats the original Giant and earns XP',!state.diagnostic.entities.some(e=>e.id===boss.id)&&state.view.character.experience>xp);
  game.check('independent Oasis chest retains its precombat contents',state.diagnostic.entities.some(e=>e.id===chest.id&&JSON.stringify(e.items)===JSON.stringify(chest.items)));
  for(let n=0;n<60;n++){
    const v=await game.observe(),dx=chest.pos.x-v.position.x,dy=chest.pos.y-v.position.y;if(Math.hypot(dx,dy)<.4)break;
    const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
    if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
    const distance=keys.length===2?Math.min(Math.abs(x),Math.abs(y))*Math.SQRT2:Math.max(Math.abs(x),Math.abs(y));
    await protect();await game.act({keys,duration_ms:Math.max(50,Math.min(250,Math.round(distance/(4+5.6*v.character.stats.speed/75)*1000)))});
  }
  const arrived=await game.observe();game.check('ordinary native walking reaches the Oasis chest',Math.hypot(arrived.position.x-chest.pos.x,arrived.position.y-chest.pos.y)<.4);
  await game.act({keys:[],duration_ms:700},{name:'oasis-chest',when:()=>true});
  const before=await game.observe(),item=chest.items[0];assert(item!==-1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  const picked=await save('picked');
  game.check('native drag commits one chest item',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===before.character.inventory_revision+1&&picked.diagnostic.entities.find(e=>e.id===chest.id)?.items[0]===-1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:678,y:428}}});
  game.check('repeating an empty chest drag cannot duplicate loot',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  await game.act({keys:['Insert'],duration_ms:350});const returned=await game.observe();
  game.check('ordinary Nexus return retains the same living character and chest item',returned.world==='Nexus'&&returned.lifecycle.alive&&returned.session.character===initial.view.session.character&&returned.character.items[4]===item&&returned.character.inventory_revision===picked.view.character.inventory_revision);
}
