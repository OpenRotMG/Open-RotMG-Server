import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const out=process.env.QA_RESULTS_DIR,bit=n=>1<<(n-1);
const spec={class:0x31d,level:20,seed:825,world:1,equipment:[0xb40,-1,0xa7b,-1],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,0]};
let placement;
export async function prepare(api){
  const world=await api({op:'world_info',world:1});
  const events=(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
  placement=events.findLast(f=>f.message==='named Realm boss setpiece placed'&&f.kind===0x935&&f.layout===`Some(${JSON.stringify(world.realm_layout)})`);assert(placement,'Ordinarily placed Crystal required');
  await writeFile(join(out,'placement.json'),JSON.stringify({world,placement}));
  return {...spec,position:{x:placement.x+2.5,y:placement.y+2.5}};
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const records=[],protectedUntil=new Map();let mystic;
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id),kinds=(s,k)=>s.diagnostic.entities.filter(e=>e.kind===k);
  async function save(label){const state=await game.inspect();records.push({label,state});await writeFile(join(out,'encounter.json'),JSON.stringify(records));return state;}
  async function cheat(actor,change){
    const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:actor.actor,change}}),signal:AbortSignal.timeout(10000)});
    const receipt=await r.json();assert(r.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function protect(actor){const now=(await actor.observe()).game_ms;if(now>=(protectedUntil.get(actor.actor)??0)){await cheat(actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});protectedUntil.set(actor.actor,now+60000);}}
  async function idle(ms){while(ms>0){for(const a of [game,mystic].filter(Boolean))await protect(a);const now=(await game.observe()).game_ms,step=Math.min(ms,5000,...[...protectedUntil.values()].map(t=>t-now));assert(step>0);await game.act({keys:[],duration_ms:step});ms-=step;}}
  async function approach(actor,id,range=4){
    for(let n=0;n<80;n++){
      await protect(actor);const p=typeof id==='number'?entity(await actor.inspect(),id)?.pos:id;assert(p,'Living walking target required');
      const v=await actor.observe(),dx=p.x-v.position.x,dy=p.y-v.position.y;if(Math.hypot(dx,dy)<range)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await actor.act({keys,duration_ms:Math.max(40,Math.min(200,Math.round(Math.hypot(dx,dy)/(4+5.6*v.character.stats.speed/75)*500)))});
    }throw Error('Native walk did not reach target');
  }
  async function fire(id,ms=150){await approach(game,id);return game.act({keys:[],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});}
  await protect(game);let state=await save('initial');
  const crystal=kinds(state,0x935)[0];assert(crystal);
  game.check('one natural Mountains Crystal has selected million HP',kinds(state,0x935).length===1&&crystal.hp===1000000&&crystal.pos.x===placement.x+2.5&&crystal.pos.y===placement.y+2.5);
  await game.act({keys:[],duration_ms:600},{name:'natural-crystal',when:v=>v.nearby.some(e=>e.kind===0x935)});
  for(let n=0;n<10&&entity(await game.inspect(),crystal.id).stage<3;n++)await fire(crystal.id);
  // Heal runs on evaluation entry, not five seconds later: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/behaviors/Heal.cs
  state=await save('evaluation');game.check('native damage admits evaluation and first heal restores Crystal',entity(state,crystal.id).stage===3&&entity(state,crystal.id).hp===1000000);
  await fire(crystal.id);state=await save('damaged');game.check('Crystal takes native damage during evaluation',entity(state,crystal.id).hp<1000000);
  await idle(5100);state=await save('healed');game.check('five-second evaluation heal restores Crystal',entity(state,crystal.id).hp===1000000);
  const evaluation=entity(state,crystal.id).activated;
  await idle(Math.max(0,evaluation+60200-state.view.game_ms));state=await save('failed');
  game.check('insufficient damage reaches failure without releasing Prisoner',entity(state,crystal.id).stage===6&&!kinds(state,0x941).length);
  game.check('failure emits selected Quiet projectiles',state.view.projectiles.some(p=>p.owner===crystal.id&&p.speed===10&&p.lifetime_ms===6000));
  await idle(5100);state=await save('reset');game.check('failed Crystal resets healed and vulnerable',entity(state,crystal.id).stage===1&&entity(state,crystal.id).hp===1000000&&!(entity(state,crystal.id).condition&bit(25)));
  for(let n=0;n<20&&entity(await game.inspect(),crystal.id)?.stage!==4;n++)await fire(crystal.id,500);
  state=await save('cracked');game.check('native burst crosses five-percent threshold into shielded break',entity(state,crystal.id)?.stage===4&&!!(entity(state,crystal.id).condition&bit(25)));
  await idle(4300);state=await save('released');const prisoner=kinds(state,0x941)[0];assert(prisoner);
  game.check('release replaces Crystal with shielded Prisoner and clears damage credit',!entity(state,crystal.id)&&prisoner.hp===27000&&!!(prisoner.condition&bit(25))&&Object.keys(prisoner.damage).length===0);
  await idle(1000);state=await save('steeds');game.check('Prisoner produces three owned Steeds',kinds(state,0x943).filter(e=>e.quest===prisoner.quest).length===3);
  await approach(game,prisoner.id,5);await game.act({keys:[],duration_ms:700},{name:'prisoner-released',when:v=>v.nearby.some(e=>e.kind===0x941)});
  const samples=[];
  for(let n=0;n<46;n++){await idle(400);samples.push(await game.inspect());}
  await writeFile(join(out,'daisy.json'),JSON.stringify(samples));
  game.check('Daisy moves and alternates shield/vulnerability',samples.some(s=>entity(s,prisoner.id)?.stage===1&&!(entity(s,prisoner.id).condition&bit(25)))&&samples.some(s=>entity(s,prisoner.id)?.stage===1&&(entity(s,prisoner.id).condition&bit(25)))&&samples.some(s=>Math.hypot(entity(s,prisoner.id).pos.x-prisoner.pos.x,entity(s,prisoner.id).pos.y-prisoner.pos.y)>.2));
  for(const [owner,speed] of [[prisoner.id,80],[prisoner.id,47]])game.check('Daisy projectile speed '+speed,samples.some(s=>s.view.projectiles.some(p=>p.owner===owner&&p.speed===speed)));
  game.check('Steeds move and fire selected bolts',samples.some(s=>s.view.projectiles.some(p=>kinds(s,0x943).some(e=>e.id===p.owner)&&p.speed===67)));
  for(let n=0;n<15&&entity(await game.inspect(),prisoner.id).stage!==3;n++)await idle(500);
  await idle(1500);state=await save('clones');const clones=kinds(state,0x942).filter(e=>e.quest===prisoner.quest);
  game.check('clone phase admits four spawned and four tossed owned Clones',clones.length===8&&clones.every(e=>e.hp===15000));
  mystic=await game.join({class:0x323,level:20,seed:826,world:1,position:state.view.position,equipment:[-1,0xb2d,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,60,25,60,50,40,75]});await protect(mystic);
  for(const kind of [0x942,0x943]){
    state=await game.inspect();const child=kinds(state,kind).find(e=>!(e.condition&(bit(22)|bit(23)))) ;assert(child,'Susceptible living child required');
    await approach(mystic,child.id,5);
    await mystic.act({keys:[' '],duration_ms:150,target:{entity:child.id,camera_angle:-Math.PI/4,fire:false}});
    state=await save('stasis-'+kind);game.check(kind+': native orb applies Stasis to loaded minion',!!(entity(state,child.id).condition&bit(22)));
  }
  state=await game.inspect();await approach(mystic,prisoner.id,5);await mystic.act({keys:[' '],duration_ms:150,target:{entity:prisoner.id,camera_angle:-Math.PI/4,fire:false}});
  state=await save('boss-immune');game.check('loaded Prisoner remains Stasis immune',!(entity(state,prisoner.id).condition&bit(22)));
  for(let n=0;n<40&&entity(await game.inspect(),prisoner.id).stage!==5;n++)await idle(500);
  state=await save('whoa');game.check('timed cycle reaches Whoa phase',entity(state,prisoner.id).stage===5);
  await approach(game,prisoner.id,1);
  const late=[];for(let n=0;n<26;n++){await idle(500);late.push(await game.inspect());}
  await writeFile(join(out,'late-attacks.json'),JSON.stringify(late));
  game.check('late phases fire selected star-speed projectiles',late.some(s=>s.view.projectiles.some(p=>p.owner===prisoner.id&&p.speed===60&&p.lifetime_ms===1700)));
  for(const [condition,name] of [[4,'Slowed'],[11,'Confused']])game.check('native star hits apply '+name,late.some(s=>s.view.character.conditions&bit(condition)));
  game.check('Massive phase grows selected boss',late.some(s=>entity(s,prisoner.id).stage===6&&entity(s,prisoner.id).size===260));
  for(let n=0;n<40&&entity(await game.inspect(),prisoner.id).stage!==7;n++)await idle(500);
  await idle(3200);state=await save('cycle');game.check('full cycle returns to Daisy and old Clones decay',entity(state,prisoner.id).stage===1&&!clones.some(c=>entity(state,c.id)));
  const xp=state.view.character.experience;
  for(let n=0;n<100&&entity(await game.inspect(),prisoner.id);n++)await fire(prisoner.id,250);
  state=await save('defeated');game.check('native combat defeats Prisoner and grants eligible XP',!entity(state,prisoner.id)&&state.view.character.experience>xp);
  await idle(2000);state=await save('cleanup');game.check('defeat removes owned Steeds without respawning Crystal',!kinds(state,0x943).some(e=>e.quest===prisoner.quest)&&!kinds(state,0x935).length);
  const bag=state.diagnostic.entities.find(e=>e.items?.some(i=>i!==-1)&&e.extra_stats.some(([s,o])=>s===54&&o===state.view.session.account));assert(bag,'Observed private reward required');
  await approach(game,bag.pos,.4);await game.act({keys:[],duration_ms:700},{name:'prisoner-reward',when:()=>true});
  const before=await game.observe(),slot=bag.items.findIndex(i=>i!==-1),item=bag.items[slot];
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+slot%4*44,y:528+Math.floor(slot/4)*44},to:{x:634,y:428}}});state=await save('picked');
  game.check('native pickup commits private reward once',state.view.character.items[4]===item&&state.view.character.inventory_revision===before.character.inventory_revision+1&&entity(state,bag.id).items[slot]===-1);
  const earned=state.view.character.experience;await idle(1000);game.check('Prisoner defeat XP is once only',(await game.observe()).character.experience===earned);
  await game.act({keys:['Insert'],duration_ms:250});const retained=await game.observe();game.check('Nexus reconnect retains living character and reward',retained.world==='Nexus'&&retained.session.character===state.view.session.character&&retained.character.items[4]===item&&retained.lifecycle.alive);
}
