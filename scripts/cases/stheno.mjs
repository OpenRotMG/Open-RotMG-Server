import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

// Independent expectations read by hand from recovered Stheno, game/123.5.1/actors.xml:3046-3120 (snakePit.xml:173).
export const SOURCE={
  order:['wait_for_pets','attack1','summon_snakes','chance_to_clear_snakes','attack2'],
  // Transition afterTime per state; attack1/attack2 alternate unshielded/shielded children.
  durations:{wait_for_pets:3000,attack1:8000,summon_snakes:12000,chance_to_clear_snakes:5000},
  // MakeMinions maxChildren: Stheno Pet 4, Stheno Swarm 6.
  pets:4,swarm:6,
  // attack1 fires 2+4 projectile-0 shots every 0.7s for 8s: roughly 69-84 shots.
  attack1Shots:[40,110],
  // Snake Eye Ring row prob .10; Potion of Speed prob .3 min 1 guarantees one for a sole eligible damager.
  ring:{probability:0.1,guaranteed:false},
};
const PHASES=new Set(['start_the_fun','wait_for_pets','attack1','summon_snakes','chance_to_clear_snakes','attack2']);
const SHIELDED=new Set(['start_the_fun','wait_for_pets','shielded1','shielded3','shielded2','chance_to_clear_snakes']);
const SPEED=0xa21,RING=0xa41,PET=0x918,SWARM=0x919,INVULNERABLE=1<<24;

export const fixture={class:0x30e,level:20,seed:855,world:'snake-pit',worldSeed:855,position:'spawn',equipment:[0xb08,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(observer){await sthenoJourney(observer,SOURCE);}

export async function sthenoJourney(observer,expect){
  const root=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(root,'native/native.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(config.state,'control.token'),'utf8')).trim(),'Content-Type':'application/json'};
  let serial=0;
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:'stheno-'+(++serial),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status!=='failed',JSON.stringify(s));return s;};
  const rates=await (await fetch(config.origin+'/api/drop-rates',{signal:AbortSignal.timeout(10000)})).json();
  const ringRow=rates.objects.find(o=>o.id===0x917).rows.find(r=>r.source_row.pool.includes(RING));
  observer.check('advertised Stheno Snake Eye Ring row matches the selected XML',ringRow.bernoulli_attempt_probability===expect.ring.probability);
  const origin=await observer.observe();
  const map=async(player=origin.session.player)=>{const r=await fetch(config.origin+'/dev/map?player='+player,{headers,signal:AbortSignal.timeout(10000)});assert(r.ok);return r.json();};
  const layout=await map(),boss=layout.entities.find(e=>e.kind===0x917);assert(boss,'Generated Stheno missing');
  const {worldSeed,...spec}=fixture;
  const game=await observer.join({...spec,world:origin.session.world,position:{x:boss.pos.x-3,y:boss.pos.y}});
  const save=async name=>{const s=await game.inspect();await writeFile(join(root,name+'.json'),JSON.stringify(s));return s;};
  const initial=await save('stheno-arrival');
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  game.check('generated Stheno starts at selected 7500 HP',entity(initial,boss.id)?.hp===7500);
  let protectedUntil;
  const protect=async()=>{await api({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});protectedUntil=(await game.observe()).game_ms+60000;};
  await protect();
  const samples=[],shots=new Map();let pending,error;
  const record=s=>{samples.push(s);for(const p of s.view.projectiles)if(!shots.has(p.owner+':'+p.id))shots.set(p.owner+':'+p.id,{...p,sample:samples.length-1});};
  async function sample(input,capture){
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>pending=null);},100);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;
  }
  for(let n=0;n<6;n++)await sample({keys:[],duration_ms:6000},n===1?{name:'stheno-guardians',when:()=>true}:undefined);
  const xml=s=>s.diagnostic.xml[boss.id],names=s=>xml(s)?.states.map(x=>x.name)??[];
  const phase=s=>names(s).find(n=>PHASES.has(n)),leaf=s=>names(s).at(-1);
  const live=samples.filter(s=>xml(s));
  const visits=[];for(const s of live){const p=phase(s);if(p!=='start_the_fun'&&p!==visits.at(-1)?.phase)visits.push({phase:p,at:xml(s).active_ms,sample:samples.indexOf(s)});}
  const order=visits.map(v=>v.phase);
  const attack1=samples.map((_,i)=>i).filter(i=>i>=visits[1].sample&&i<visits[2].sample);
  const attack1Shots=[...shots.values()].filter(p=>p.owner===boss.id&&p.speed===70&&attack1.includes(p.sample)).length;
  const pets=samples.map(s=>s.diagnostic.entities.filter(e=>e.kind===PET&&e.parent===boss.id));
  const swarms=samples.map(s=>s.diagnostic.entities.filter(e=>e.kind===SWARM&&e.parent===boss.id).length);
  const firstSummon=live.findIndex(s=>phase(s)==='summon_snakes');
  const shield=live.map(s=>[SHIELDED.has(leaf(s)),Boolean((s.view.objects.find(o=>o.id===boss.id)?.condition_mask??0)&INVULNERABLE)]);
  const bossMoves=live.map(s=>Math.hypot(entity(s,boss.id).pos.x-boss.pos.x,entity(s,boss.id).pos.y-boss.pos.y));
  const fingerprints=new Set(live.map(s=>xml(s).input_fingerprint));
  const summary={expect,order,visits,attack1Shots,maxPets:Math.max(...pets.map(p=>p.length)),maxSwarm:Math.max(...swarms),shieldAgreement:shield.filter(([a,b])=>a===b).length/shield.length,bossMaxDistance:Math.max(...bossMoves),fingerprints:[...fingerprints]};
  await writeFile(join(root,'stheno-phases.json'),JSON.stringify({boss,summary,samples,shots:[...shots.values()]}));
  await writeFile(join(root,'stheno-summary.json'),JSON.stringify(summary,null,1));
  game.check('every sample runs one XML GameData fingerprint',fingerprints.size===1);
  game.check('Stheno follows the selected XML state order',JSON.stringify(order.slice(0,expect.order.length))===JSON.stringify(expect.order));
  game.check('XML afterTime transitions hold within two 200ms behavior units',expect.order.slice(0,-1).every((p,i)=>Math.abs(visits[i+1].at-visits[i].at-expect.durations[p])<=400));
  game.check('XML shield states drive client-visible invulnerability',summary.shieldAgreement>=.9&&shield.some(([a])=>a)&&shield.some(([a])=>!a));
  game.check('first attack1 projectile-0 volume matches its XML cooldown',attack1Shots>=expect.attack1Shots[0]&&attack1Shots<=expect.attack1Shots[1]);
  const family=(speed,lifetime)=>[...shots.values()].some(p=>p.owner===boss.id&&p.speed===speed&&p.lifetime_ms===lifetime);
  game.check('native client sees selected projectile 0, and projectile 1 only when attack2 runs',family(70,1500)&&family(62,2000)===expect.order.includes('attack2'));
  game.check('guardians reach exactly the XML MakeMinions bound and move',summary.maxPets===expect.pets&&pets.find(p=>p.length===expect.pets).every(p=>pets.some(q=>q.some(e=>e.id===p.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>1))));
  game.check('swarm appears only from summon_snakes and stays within its XML bound',firstSummon>0&&swarms.slice(0,samples.indexOf(live[firstSummon])).every(n=>n===0)&&swarms.some(n=>n>0)&&summary.maxSwarm<=expect.swarm);
  game.check('Stheno moves and stays near spawn (StayCloseToSpawn range 4)',summary.bossMaxDistance>.3&&summary.bossMaxDistance<=5);
  // Protection is capped at 60s and cannot overlap, so the fight renews it right after it expires.
  for(let n=0;n<120;n++){
    const now=(await game.observe()).game_ms;
    if(now>=protectedUntil)await protect();
    const s=await game.inspect(),current=entity(s,boss.id);
    if(!current)break;
    // A previous shot can defeat Stheno between inspection and the next input. Aim at the last visible bearing so the controller does not try to reacquire a retired actor.
    const target=s.view.objects.find(e=>e.id===boss.id&&e.hp>0);
    let mouse;
    if(target){
      const {x,y}=s.view.position,dx=target.position.x-x,dy=target.position.y-y;
      const scale=35.355/Math.max(1,Math.hypot(dx,dy)*50/90);
      mouse={x:300+(dx-dy)*scale,y:325+(dx+dy)*scale};
    }
    await game.act({keys:[],duration_ms:Math.min(1000,Math.max(50,protectedUntil-now+50)),mouse});
  }
  await game.act({keys:[],duration_ms:500});
  const defeated=await save('stheno-defeated'),cleared=await map(defeated.view.session.player);
  game.check('ordinary native staff defeats Stheno and grants XP',!cleared.entities.some(e=>e.id===boss.id)&&defeated.view.character.experience>initial.view.character.experience);
  const mine=cleared.entities.filter(e=>e.items&&e.extra_stats?.some(([stat,owner])=>stat===54&&owner===defeated.view.session.account));
  const items=mine.flatMap(e=>e.items).filter(i=>i>=0);
  game.check('sole damager receives exactly one Speed (prob .3 min 1)',items.filter(i=>i===SPEED).length===1);
  if(expect.ring.guaranteed)game.check('changed XML ring row is delivered',items.includes(RING));
  const bag=mine.find(e=>e.items.includes(SPEED));
  const toward=(a,b)=>{const dx=b.x-a.x,dy=b.y-a.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;return [Math.abs(x)>.15&&Math.abs(x)>=Math.abs(y)*.4142?(x>0?'d':'a'):null,Math.abs(y)>.15&&Math.abs(y)>=Math.abs(x)*.4142?(y>0?'s':'w'):null].filter(Boolean);};
  async function walk(point){
    for(let n=0;n<60;n++){const v=await game.observe(),d=Math.hypot(point.x-v.position.x,point.y-v.position.y);if(d<.4)return;await game.act({keys:toward(v.position,point),duration_ms:Math.max(50,Math.min(200,Math.round(d/(4+5.6*v.character.stats.speed/75)*1000)))});}
    throw Error('Native approach failed: '+JSON.stringify(point));
  }
  await walk(bag.pos);
  await game.act({keys:[],duration_ms:1000},{name:'stheno-reward',when:()=>true});
  const slot=bag.items.indexOf(SPEED),before=await game.observe();
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+(slot%4)*44,y:528+Math.floor(slot/4)*44},to:{x:634,y:428}}});
  const picked=await save('stheno-picked');
  game.check('native drag acquires earned Speed once',picked.view.character.items[4]===SPEED&&picked.view.character.inventory_revision===before.character.inventory_revision+1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+(slot%4)*44,y:528+Math.floor(slot/4)*44},to:{x:678,y:428}}});
  game.check('empty reward slot cannot duplicate Speed',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  const portal=cleared.entities.find(e=>[0x704,0x70e].includes(e.kind));assert(portal,'Boss return portal missing');
  await walk(portal.pos);
  await game.act({keys:[],duration_ms:800},{name:'stheno-exit',when:()=>true});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
  for(let n=0;n<8&&(await game.observe()).session.world===origin.session.world;n++)await game.act({keys:[],duration_ms:500});
  const returned=await save('stheno-returned');
  game.check('earned exit returns the same living character with its reward',returned.view.map.name==='Realm'&&returned.view.session.character===initial.view.session.character&&returned.view.lifecycle.alive&&returned.view.character.items[4]===SPEED);
  await game.act({keys:[],duration_ms:2500});
  await game.act({keys:['d'],duration_ms:300});
  const final=await save('stheno-final');
  game.check('return admission finishes and movement works',Math.hypot(final.view.position.x-returned.view.position.x,final.view.position.y-returned.view.position.y)>.1);
  game.check('Stheno XP is not awarded repeatedly',final.view.character.experience===defeated.view.character.experience);
  game.check('committed boss clear is counted once and survives the exit',defeated.view.character.snake_pits===initial.view.character.snake_pits+1&&picked.view.character.snake_pits===defeated.view.character.snake_pits&&returned.view.character.snake_pits===defeated.view.character.snake_pits&&final.view.character.snake_pits===returned.view.character.snake_pits);
  await writeFile(join(root,'stheno-outcome.json'),JSON.stringify({boss,bag,portal,initial,defeated,picked,returned,final,observer:await observer.inspect()}));
}
