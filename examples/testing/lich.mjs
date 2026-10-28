import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const out=process.env.QA_RESULTS_DIR,mode=process.env.LICH_BRANCH;
const branches=['Solo','SmallGroup','Mob','HugeMob'];
let branch=mode==='Remaining'?'SmallGroup':mode,index=branches.indexOf(branch),roots;
const spec={class:0x31d,level:20,seed:793,world:1,equipment:[0xb0b,0xb29,0xa7b,-1],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,0]};
let root;
export async function prepare(api){
  assert(index>=0,'Set LICH_BRANCH to Solo, SmallGroup, Mob or HugeMob');
  const world=await api({op:'world_info',world:1});
  const events=(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
  const end=events.findLastIndex(f=>f.message==='Realm simulation populated'&&f.layout===world.realm_layout);assert(end>=0);
  const start=events.slice(0,end).findLastIndex(f=>f.message==='Realm simulation populated');
  roots=events.slice(start+1,end).filter(f=>f.message==='finite named Realm quest placed'&&f.kind===0x91b&&f.layout===world.realm_layout);
  assert(roots.length>=4,'Four ordinarily placed Liches required');root=roots[index];
  await writeFile(join(out,'placement.json'),JSON.stringify({world,roots,branch}));
  return {...spec,position:{x:root.x,y:root.y}};
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const records=[],protectedUntil=new Map();let partner,attackUntil=0;
  async function cheat(actor,change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:actor.actor,change}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function protect(actor){
    const now=(await actor.observe()).game_ms;
    if(now>=(protectedUntil.get(actor.actor)??0)){await cheat(actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});protectedUntil.set(actor.actor,now+60000);}
  }
  async function idle(ms){
    while(ms>0){const actors=[game,partner].filter(Boolean);for(const actor of actors)await protect(actor);const now=(await game.observe()).game_ms,step=Math.min(ms,5000,...actors.map(actor=>protectedUntil.get(actor.actor)-now));await game.act({keys:[],duration_ms:step});ms-=step;}
  }
  const entity=(state,id)=>state.diagnostic.entities.find(e=>e.id===id);
  const family=state=>state.diagnostic.entities.filter(e=>e.quest===root.entity);
  async function save(label){const state=await game.inspect();records.push({branch,label,state});await writeFile(join(out,'encounters.json'),JSON.stringify(records));return state;}
  async function approach(actor,point,range=1.5){
    for(let n=0;n<60;n++){
      await protect(actor);const view=await actor.observe(),dx=point.x-view.position.x,dy=point.y-view.position.y;if(Math.hypot(dx,dy)<range)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');
      if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      const distance=keys.length===2?Math.min(Math.abs(x),Math.abs(y))*Math.SQRT2:Math.max(Math.abs(x),Math.abs(y));
      await actor.act({keys,duration_ms:Math.max(50,Math.min(250,Math.round(distance/(4+5.6*view.character.stats.speed/75)*1000)))});
    }
    throw Error('Native walking did not reach the observed target');
  }
  async function fire(actor,id,ms=150){
    await protect(actor);const target=entity(await actor.inspect(),id);assert(target,'Living native target required');
    await approach(actor,target.pos);
    return actor.act({keys:[],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
  }
  const indices=mode==='Remaining'?[1,2,3]:[index];
  for(const i of indices){
  index=i;branch=branches[i];root=roots[i];
  if(i!==indices[0]){await cheat(game,{kind:'teleport',position:{x:root.x,y:root.y}});await game.act({keys:[],duration_ms:100});}
  const initial=await save('idle');await protect(game);
  game.check('untouched naturally placed Lich starts vulnerable at selected HP',entity(initial,root.entity)?.hp===300000&&!(entity(initial,root.entity).condition&(1<<24)));
  await game.act({keys:['s'],duration_ms:150});
  game.check('ordinary movement works at the natural ruin',Math.hypot((await game.observe()).position.x-initial.view.position.x,(await game.observe()).position.y-initial.view.position.y)>.3);
  await approach(game,initial.view.position,.3);
  if(index===3){partner=await game.join({...spec,seed:794,position:initial.view.position});await protect(partner);}
  if(index>0){
    await idle(Math.max(0,attackUntil-(await game.observe()).game_ms));
    for(const actor of [game,partner].filter(Boolean))await cheat(actor,{kind:'stat_modifier',index:2,amount:100,duration_ms:60000});
    attackUntil=(await game.observe()).game_ms+60000;
  }
  for(let n=0;n<20&&entity(await game.inspect(),root.entity).stage!==1;n++)await fire(game,root.entity);
  let state=await save('admitted');
  game.check('native sword damage admits the shielded evaluation introduction',entity(state,root.entity).stage===1&&!!(entity(state,root.entity).condition&(1<<24)));
  if(index>0){
    const sequence=[{actor:game.actor,input:{keys:[],duration_ms:2000}},{actor:game.actor,input:{keys:[' '],duration_ms:250}}];
    for(const actor of [game,partner].filter(Boolean))if(actor!==game)sequence.push({actor:actor.actor,input:{keys:[' '],duration_ms:250}});
    const burst=[0,700,2500,4000][index];
    for(let ms=0;ms<burst;ms+=250){const input={keys:[],duration_ms:Math.min(250,burst-ms),target:{entity:root.entity,camera_angle:-Math.PI/4,fire:true}};sequence.push(partner?[{actor:game.actor,input},{actor:partner.actor,input}]:{actor:game.actor,input});}
    await game.act(sequence);
  }
  for(let n=0;n<10&&entity(await game.inspect(),root.entity)?.stage<3;n++)await idle(1000);
  state=await save('selected');
  game.check(branch+' selected from observed combat damage',entity(state,root.entity)?.stage===3+index);
  const selected=entity(state,root.entity),xp=state.view.character.experience;
  game.check('preparation is vulnerable and transformation has not awarded XP',!(selected.condition&(1<<24))&&xp===initial.view.character.experience);
  if(index>0){
    game.check('owned bearer first wave matches branch',[2,3,5][index-1]===family(state).filter(e=>e.kind===0x91d).length);
    await idle(1000);state=await save('healed');
    game.check('living owned bearers restore their damaged Lich',entity(state,root.entity).hp===300000&&Object.values(entity(state,root.entity).damage).reduce((a,b)=>a+b,0)>4500);
    if(index===1)await idle(Math.max(0,selected.activated+15000-(await game.observe()).game_ms));else await idle(5000);
    state=await save('spirit-shield');const spirit=family(state).find(e=>e.kind===0x91e);assert(spirit,'Owned Haunted Spirit required');
    game.check('summoned Spirit starts invulnerable',!!(spirit.condition&(1<<24)));
    await idle(Math.max(0,spirit.activated+7500-(await game.observe()).game_ms));state=await save('spirit-vulnerable');
    game.check('same Spirit becomes vulnerable during its attack window',!(entity(state,spirit.id).condition&(1<<24)));
  }
  for(let n=0;n<9&&entity(await game.inspect(),root.entity);n++)await idle(5000);
  state=await save('actual');const actual=family(state).find(e=>e.kind===0x91c);assert(actual,'Actual Lich required');
  game.check('transformation preserves encounter ownership without XP',!entity(state,root.entity)&&actual.quest===root.entity&&state.view.character.experience===xp);
  await approach(game,actual.pos);
  await game.act({keys:[],duration_ms:750},{name:branch+'-actual',when:view=>view.nearby.some(e=>e.kind===0x91c)});
  await idle(16000);state=await save('mummies');
  game.check('all three owned mummy variants coexist',[0x65f,0x660,0x661].every(k=>family(state).some(e=>e.kind===k)));
  const samples=[];
  for(let n=0;n<10;n++){await idle(1000);samples.push(await game.inspect());}
  await writeFile(join(out,'attacks.json'),JSON.stringify(samples));
  for(const [kind,speed,lifetime] of [[0x65f,70,3000],[0x660,100,2000],[0x661,70,3000]]){
    game.check(kind+': native scene contains its distinct selected projectile',samples.some(s=>s.view.projectiles.some(p=>family(s).some(e=>e.kind===kind&&e.id===p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime)));
  }
  for(const speed of [80,60])game.check('Actual Lich fires projectile speed '+speed,samples.some(s=>s.view.projectiles.some(p=>p.owner===actual.id&&p.speed===speed)));
  game.check('Actual Lich moves while attacking',samples.some(s=>{const e=entity(s,actual.id);return e&&Math.hypot(e.pos.x-actual.pos.x,e.pos.y-actual.pos.y)>.2;}));
  state=await save('attacks-observed');
  game.check('encounter stays within per-kind population bounds',[[0x65f,4],[0x660,2],[0x661,1],[0x91d,8],[0x91e,[0,1,2,5][index]]].every(([k,max])=>family(state).filter(e=>e.kind===k).length<=max));
  if(index>0){
    const bearer=family(state).find(e=>e.kind===0x91d);assert(bearer);await fire(game,bearer.id,150);await idle(500);state=await save('bearer-damaged');
    game.check('native sword fire damages a vulnerable bearer',entity(state,bearer.id)?.hp<6000);
  }
  for(let n=0;n<60;n++){
    state=await game.inspect();const bearer=family(state).find(e=>e.kind===0x91d);if(!bearer)break;await fire(game,bearer.id,250);
  }
  game.check('native combat defeats every owned healer',!family(await game.inspect()).some(e=>e.kind===0x91d));
  for(let n=0;n<30&&entity(await game.inspect(),actual.id);n++)await fire(game,actual.id,150);
  state=await save('defeated');
  game.check('Actual Lich is defeated by native sword fire',!entity(state,actual.id));
  game.check('eligible player receives defeat experience',state.view.character.experience>xp);
  const earned=state.view.character.experience;await idle(2000);
  game.check('defeat credit is once only',(await game.observe()).character.experience===earned);
  state=await save('rewards');
  const bag=state.diagnostic.entities.find(e=>e.items?.some(i=>i!==-1)&&e.extra_stats.some(([stat,owner])=>stat===54&&owner===state.view.session.account));
  assert(bag,'This observed roll must contain a private reward for the pickup check');
  await approach(game,bag.pos,.4);
  await game.act({keys:[],duration_ms:700},{name:branch+'-reward',when:()=>true});
  const inventory=(await game.observe()).character,slot=bag.items.findIndex(i=>i!==-1),item=bag.items[slot],destination=inventory.items.findIndex((v,n)=>n>=4&&v===-1);assert(destination>=4,'Free inventory slot required');
  const to={x:634+(destination%4)*44,y:428+Math.floor((destination-4)/4)*44};
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+(slot%4)*44,y:528+Math.floor(slot/4)*44},to}});
  const picked=await save('picked');
  game.check('native drag commits one eligible reward',picked.view.character.items[destination]===item&&picked.view.character.inventory_revision===inventory.inventory_revision+1&&entity(picked,bag.id)?.items[slot]===-1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+(slot%4)*44,y:528+Math.floor(slot/4)*44},to:{x:678,y:428}}});
  game.check('empty-slot drag cannot duplicate the reward',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  if(i===indices.at(-1)){
    await game.act({keys:['Insert'],duration_ms:250});
    const retained=await game.observe();
    game.check('Nexus reconnect preserves the living character and every collected reward',retained.world==='Nexus'&&retained.lifecycle.alive&&retained.session.character===initial.view.session.character&&JSON.stringify(retained.character.items)===JSON.stringify(picked.view.character.items)&&retained.character.inventory_revision===picked.view.character.inventory_revision);
  }
  await writeFile(join(out,branch+'-branch.json'),JSON.stringify({branch,root,actual,earned,picked,retained:await game.observe()}));
  }
}
