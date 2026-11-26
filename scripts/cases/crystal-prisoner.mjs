import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {journaledReward} from './reward-journal.mjs';
process.env.RUST_LOG='info,openrotmg::world::named_births=debug,openrotmg::world::native_hits=debug';
const out=process.env.QA_RESULTS_DIR,bit=n=>1<<(n-1);
const spec={class:0x31d,level:20,seed:825,world:1,equipment:[0xb40,-1,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,0]};
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
  // Recovered top-level states: Crystal Idle/Instructions/Evaluation/Fail/StartBreak/BreakCrystal (actors.xml:4203), Prisoner Daisy_attack/Summon_the_clones/Whoa_nelly/Absolutely_Massive/Start_over_again (actors.xml:4365).
  const top=(s,id)=>s.diagnostic.xml[id]?.states[1]?.name;
  function flags(s,id){const mask=s.view.objects.find(e=>e.id===id)?.condition_mask;assert(Number.isInteger(mask),'Published target conditions required');return mask;}
  async function save(label){const state=await game.inspect();records.push({label,state});await writeFile(join(out,'encounter.json'),JSON.stringify(records));return state;}
  async function cheat(actor,change){
    const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:actor.actor,change}}),signal:AbortSignal.timeout(10000)});
    const receipt=await r.json();assert(r.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function protect(actor){const now=(await actor.observe()).game_ms;if(now>=(protectedUntil.get(actor.actor)??0)){await cheat(actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});protectedUntil.set(actor.actor,now+60000);}}
  async function idle(ms){assert(Number.isFinite(ms)&&ms>=0);const end=(await game.observe()).game_ms+ms;while((await game.observe()).game_ms<end){for(const a of [game,mystic].filter(Boolean))await protect(a);const now=(await game.observe()).game_ms,step=Math.min(end-now,5000,...[...protectedUntil.values()].map(t=>t-now));assert(step>0);await game.act({keys:[],duration_ms:step});}}
  async function sample(ms,values){
    let done=false;const action=idle(ms).finally(()=>done=true),collect=(async()=>{while(!done){values.push(await game.inspect());await new Promise(ok=>setTimeout(ok,150));}})();
    for(const r of await Promise.allSettled([action,collect]))if(r.status==='rejected')throw r.reason;
  }
  async function approach(actor,id,range=4){
    for(let n=0;n<80;n++){
      for(const a of [game,mystic].filter(Boolean))await protect(a);const p=typeof id==='number'?entity(await actor.inspect(),id)?.pos:id;assert(p,'Living walking target required');
      const v=await actor.observe(),dx=p.x-v.position.x,dy=p.y-v.position.y;if(Math.hypot(dx,dy)<range)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await actor.act({keys,duration_ms:Math.max(40,Math.min(200,Math.round(Math.hypot(dx,dy)/(4+5.6*v.character.stats.speed/75)*500)))});
    }throw Error('Native walk did not reach target');
  }
  async function fire(id,ms=150){
    for(const a of [game,mystic].filter(Boolean))await protect(a);
    const p=entity(await game.inspect(),id)?.pos;assert(p,'Living native fire target required');const v=await game.observe(),dx=p.x-v.position.x,dy=p.y-v.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
    if(Math.hypot(dx,dy)>1&&!(v.character.conditions&bit(11))){if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');}
    return game.act({keys,duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
  }
  await protect(game);let state=await save('initial');
  const crystal=kinds(state,0x935)[0];assert(crystal);
  game.check('one natural Mountains Crystal has selected million HP',kinds(state,0x935).length===1&&crystal.hp===1000000&&crystal.pos.x===placement.x+2.5&&crystal.pos.y===placement.y+2.5);
  await game.act({keys:[],duration_ms:600},{name:'natural-crystal',when:v=>v.nearby.some(e=>e.kind===0x935)});
  for(let n=0;n<10&&top(await game.inspect(),crystal.id)!=='Evaluation';n++)await fire(crystal.id,50);
  // The executor runs a state's Heal on entry (next = offset), and the Crystal heals itself as a member of Crystals (behavior.cc:1306-1317).
  // Admin Sword hits are exactly 4000, so without the entry heal Evaluation starts at or below 992000; one in-flight hit may land after it.
  state=await save('evaluation');game.check('native damage admits evaluation and first heal restores Crystal',top(state,crystal.id)==='Evaluation'&&entity(state,crystal.id).hp>992000);
  await fire(crystal.id);state=await save('damaged');game.check('Crystal takes native damage during evaluation',entity(state,crystal.id).hp<1000000);
  await idle(5100);state=await save('healed');game.check('five-second evaluation heal restores Crystal',entity(state,crystal.id).hp===1000000);
  const evaluation=records.find(r=>r.label==='evaluation').state.view.time_ms;
  await idle(Math.max(0,evaluation+60200-state.view.time_ms));state=await save('failed');
  game.check('insufficient damage reaches failure without releasing Prisoner',top(state,crystal.id)==='Fail'&&!kinds(state,0x941).length);
  game.check('failure emits selected Quiet projectiles',state.view.projectiles.some(p=>p.owner===crystal.id&&p.speed===10&&p.lifetime_ms===6000));
  await idle(5100);state=await save('reset');game.check('failed Crystal resets healed and vulnerable',top(state,crystal.id)==='Idle'&&entity(state,crystal.id).hp===1000000&&!(entity(state,crystal.id).condition&bit(25)));
  game.check('native failure-ring hit applies Quiet',!!(state.view.character.conditions&bit(2))&&state.view.character.mp===0);
  // Prepared burst: the 50k (5%) threshold must land between 5s self-heals, so one continuous Admin Sword act (~17 x 4000) stands in
  // for the XML's 'large group'; 3s keeps the act inside the 4s StartBreak (a 4.5s act ran into BreakCrystal, VPS 7e1a96c1).
  for(let n=0;n<4&&top(await game.inspect(),crystal.id)!=='StartBreak';n++)await fire(crystal.id,3000);
  state=await save('cracked');game.check('prepared continuous native burst crosses five-percent threshold into shielded break',top(state,crystal.id)==='StartBreak'&&!!(entity(state,crystal.id).condition&bit(25)));
  await idle(4300);state=await save('released');const prisoner=kinds(state,0x941)[0];assert(prisoner);
  game.check('release replaces Crystal with shielded Prisoner and clears damage credit',!entity(state,crystal.id)&&prisoner.hp===27000&&!!(prisoner.condition&bit(25))&&Object.keys(prisoner.damage).length===0);
  await idle(1000);state=await save('steeds');const steeds=kinds(state,0x943);game.check('Prisoner produces three owned Steeds',steeds.filter(e=>e.parent===prisoner.id).length===3);
  await approach(game,prisoner.id,5);await game.act({keys:[],duration_ms:700},{name:'prisoner-released',when:v=>v.nearby.some(e=>e.kind===0x941)});
  const samples=[];
  await sample(Math.max(0,prisoner.activated+21500-(await game.observe()).game_ms),samples);
  await writeFile(join(out,'daisy.json'),JSON.stringify(samples));
  game.check('Daisy moves and alternates shield/vulnerability',samples.some(s=>top(s,prisoner.id)==='Daisy_attack'&&!(entity(s,prisoner.id).condition&bit(25)))&&samples.some(s=>top(s,prisoner.id)==='Daisy_attack'&&(entity(s,prisoner.id).condition&bit(25)))&&samples.some(s=>Math.hypot(entity(s,prisoner.id).pos.x-prisoner.pos.x,entity(s,prisoner.id).pos.y-prisoner.pos.y)>.2));
  for(const [owner,speed] of [[prisoner.id,80],[prisoner.id,47]])game.check('Daisy projectile speed '+speed,samples.some(s=>s.view.projectiles.some(p=>p.owner===owner&&p.speed===speed)));
  game.check('Steeds move and fire selected bolts',samples.some(s=>s.view.projectiles.some(p=>kinds(s,0x943).some(e=>e.id===p.owner)&&p.speed===67))&&samples.some(s=>steeds.some(e=>{const p=entity(s,e.id)?.pos;return p&&Math.hypot(p.x-e.pos.x,p.y-e.pos.y)>.2;})));
  game.check('native hits publish Weak during Daisy',samples.some(s=>s.view.character.conditions&bit(3)));
  for(let n=0;n<15&&top(await game.inspect(),prisoner.id)!=='Summon_the_clones';n++)await idle(500);
  await idle(1500);state=await save('clones');const clones=kinds(state,0x942).filter(e=>e.parent===prisoner.id);
  await writeFile(join(out,'clone-admission.json'),JSON.stringify({clones}));
  // Summon_the_clones makes four Clones and tosses up to four more onto placeable ground (actors.xml:4437-4441).
  game.check('clone phase produces four births plus terrain-admitted owned tosses',clones.length>=4&&clones.length<=8&&clones.every(e=>e.hp===15000));
  mystic=await game.join({class:0x323,level:20,seed:826,world:1,position:state.view.position,equipment:[-1,0xb2d,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,60,25,60,50,40,75]});await protect(mystic);
  for(const kind of [0x942,0x943]){
    state=await game.inspect();const child=kinds(state,kind).find(e=>!(flags(state,e.id)&bit(23))) ;assert(child,'Susceptible living child required');
    await approach(mystic,child.id,5);
    await mystic.act({keys:[' '],duration_ms:150,target:{entity:child.id,camera_angle:-Math.PI/4,fire:false}});
    state=await save('stasis-'+kind);const frozen=entity(state,child.id);game.check(kind+': native orb publishes eight-second Stasis on loaded minion',!!(flags(state,child.id)&bit(22))&&frozen.statuses['22'][1]-frozen.statuses['22'][0]===8000);
    await idle(500);game.check(kind+': Stasis freezes movement',JSON.stringify(entity(await game.inspect(),child.id).pos)===JSON.stringify(frozen.pos));
  }
  state=await game.inspect();await approach(mystic,prisoner.id,5);await mystic.act({keys:[' '],duration_ms:150,target:{entity:prisoner.id,camera_angle:-Math.PI/4,fire:false}});
  state=await save('boss-immune');game.check('loaded Prisoner remains Stasis immune',!(flags(state,prisoner.id)&bit(22)));
  const caster=(await mystic.observe()).session.character;await mystic.act({keys:['Insert'],duration_ms:250});
  game.check('caster escapes normally after the Stasis checks',(await mystic.observe()).world==='Nexus'&&(await mystic.observe()).session.character===caster);
  for(let n=0;n<40&&top(await game.inspect(),prisoner.id)!=='Whoa_nelly';n++)await idle(500);
  state=await save('whoa');game.check('timed cycle reaches Whoa phase',top(state,prisoner.id)==='Whoa_nelly');
  await approach(game,prisoner.id,1);
  // Whoa_nelly lasts 10s, then Absolutely_Massive grows to 260 (actors.xml:4458-4478).
  const late=[];await sample(15000,late);
  await writeFile(join(out,'late-attacks.json'),JSON.stringify(late));
  game.check('late phases fire selected star-speed projectiles',late.some(s=>s.view.projectiles.some(p=>p.owner===prisoner.id&&p.speed===60&&p.lifetime_ms===1700)));
  for(const [condition,name] of [[4,'Slowed'],[11,'Confused']])game.check('native star hits apply '+name,late.some(s=>s.view.character.conditions&bit(condition)));
  game.check('Massive phase grows selected boss',late.some(s=>top(s,prisoner.id)==='Absolutely_Massive'&&entity(s,prisoner.id).size===260));
  for(let n=0;n<40&&top(await game.inspect(),prisoner.id)!=='Start_over_again';n++)await idle(500);
  await idle(3200);state=await save('cycle');game.check('full cycle returns to Daisy and old Clones decay',top(state,prisoner.id)==='Daisy_attack'&&!clones.some(c=>entity(state,c.id)));
  const xp=state.view.character.experience;
  let rateUntil=0;
  for(let n=0;n<100&&entity(await game.inspect(),prisoner.id);n++){
    const now=(await game.observe()).game_ms;if(now>=rateUntil){await cheat(game,{kind:'stat_modifier',index:5,amount:100,duration_ms:60000});rateUntil=now+60000;}
    await fire(prisoner.id,500);
  }
  state=await save('defeated');game.check('native combat defeats Prisoner and grants eligible XP',!entity(state,prisoner.id)&&state.view.character.experience>xp);
  // Recovered Steeds have no death rule (actors.xml Crystal Prisoner Steed), so they outlive the Prisoner; base removed them.
  const orphans=new Set(kinds(state,0x943).map(e=>e.id));
  await idle(2000);state=await save('cleanup');game.check('defeat stops Steed births without respawning Crystal',kinds(state,0x943).every(e=>orphans.has(e.id))&&!kinds(state,0x935).length);
  const reward=await journaledReward(config,prisoner.id,state.view.session.character);
  game.check('journaled Prisoner allocation follows its XML private rows for the eligible contributor',reward.valid&&reward.share>=.03);
  const bags=state.diagnostic.entities.filter(e=>e.items?.some(i=>i!==-1)&&e.extra_stats.some(([s,o])=>s===54&&o===state.view.session.account)&&Math.hypot(e.pos.x-reward.position.x,e.pos.y-reward.position.y)<1);
  await writeFile(join(out,'reward.json'),JSON.stringify({reward,bags}));
  let item=-1;
  if(!reward.items.length)game.check('empty journaled roll publishes no private Prisoner bag',!bags.length);
  else{
    const bag=bags.find(b=>JSON.stringify(b.items.filter(i=>i!==-1).sort())===JSON.stringify([...reward.items].sort()));assert(bag,'Journaled Prisoner allocation must publish its private bag');
    await approach(game,bag.pos,.4);await game.act({keys:[],duration_ms:700},{name:'prisoner-reward',when:()=>true});
    const before=await game.observe(),slot=bag.items.findIndex(i=>i!==-1);item=bag.items[slot];
    await game.act({keys:[],duration_ms:350,drag:{from:{x:634+slot%4*44,y:528+Math.floor(slot/4)*44},to:{x:634,y:428}}});state=await save('picked');
    game.check('native pickup commits private reward once',state.view.character.items[4]===item&&state.view.character.inventory_revision===before.character.inventory_revision+1&&(!entity(state,bag.id)||entity(state,bag.id).items[slot]===-1));
  }
  const safe=await mystic.observe();game.check('escaped caster stays alive without acquiring a reward',safe.world==='Nexus'&&safe.lifecycle.alive&&safe.session.character===caster&&safe.character.items.slice(4).every(i=>i===-1));
  const earned=state.view.character.experience;await idle(1000);game.check('Prisoner defeat XP is once only',(await game.observe()).character.experience===earned);
  await game.act({keys:['Insert'],duration_ms:250});const retained=await game.observe();game.check('Nexus reconnect retains living character and reward',retained.world==='Nexus'&&retained.session.character===state.view.session.character&&retained.character.items[4]===item&&retained.lifecycle.alive);
}
