import assert from 'node:assert/strict';
import {nativeMap} from './native-map.mjs';
import {readFile,readdir,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {pathToFileURL} from 'node:url';
import {setTimeout as delay} from 'node:timers/promises';
const out=process.env.QA_RESULTS_DIR;
// event_rng = StdRng(seed^world^0x7265616c6d): this seed's first qualifying hero defeat wins the .25 roll and chooses 0xe37.
export const realmSeed=10;
// The Anchor stamps a NoWalk Dark Water lake (geometry.xml wp_full_lake2.jm, radius ~8.5) under the ship, so the fighter needs a
// range-10 wand from the shore; staff range ~8.5 never reached the orbiting ship (VPS 381a9521).
const spec={class:0x310,level:20,seed:823,world:1,equipment:[0xaf8,-1,0xb05,-1],backpack:Array(8).fill(-1),stats:[670,385,50,0,50,50,40,75]};
let roots,scout,scoutName,home;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];home={x,y};
  const spawned=await api({op:'spawn',spec:{...spec,guid:'ship-scout',position:{x,y},controller:'direct',profile:null}});scout=spawned.result.actor;
  const state=await api({op:'inspect',actor:scout});scoutName=state.diagnostic.character.name;assert(scoutName);
  const report=JSON.parse(await readFile(join(out,'native/result.json'),'utf8'));
  const token=(await readFile(join(out,'native/state/control.token'),'utf8')).trim();
  const map=await nativeMap(report.origin,{Authorization:'Bearer '+token},state.diagnostic.player);
  const kinds=[0x678,0x675,0x91f,0x91b,0x928];
  roots=map.entities.filter(e=>kinds.includes(e.kind)).sort((a,b)=>kinds.indexOf(a.kind)-kinds.indexOf(b.kind));assert(roots.length,'Natural Realm heroes required');
  await writeFile(join(out,'placement.json'),JSON.stringify({world,roots}));
  return {...spec,position:roots[0].pos};
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  let protectedUntil=0,scoutProtectedUntil=0,lastTeleport=-11000;const records=[];
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  async function cheat(change,actor=game.actor){const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor,change}})});const receipt=await r.json();assert(r.ok&&receipt.status==='succeeded',JSON.stringify(receipt));}
  // One prepared stat modifier at a time: +100 ATT inside protection windows, +100 DEF across each renewal gap.
  let modifierUntil=0;
  async function modifier(index,until){const now=(await game.observe()).game_ms;if(now<modifierUntil||until-now<1)return;await cheat({kind:'stat_modifier',index,amount:100,duration_ms:until-now});modifierUntil=until;}
  // Protection cannot overlap, so idle out a window the next act would cross and enter the overrun gap at full HP with +100 DEF: a lapsed
  // volley met the ship's barrage (VPS f7e05d65) and a 10x80 Water Mine explosion killed the 670-HP Priest (local 6cc46434).
  async function protect(next=0){let now=(await game.observe()).game_ms;
    if(now<protectedUntil&&protectedUntil-now<next+1500){
      if(protectedUntil-1500>now)await game.act({keys:[],duration_ms:protectedUntil-1500-now});
      await modifier(3,protectedUntil+3000);const v=await game.observe();await cheat({kind:'vitals',hp:v.character.stats.max_hp,mp:v.character.mp});
      if(v.game_ms<protectedUntil)await game.act({keys:[],duration_ms:protectedUntil-v.game_ms});now=(await game.observe()).game_ms;}
    if(now>=protectedUntil){await cheat({kind:'protection',condition:'invulnerable',duration_ms:60000});protectedUntil=now+60000;}if(now>=scoutProtectedUntil){await cheat({kind:'protection',condition:'invulnerable',duration_ms:60000},scout);scoutProtectedUntil=now+60000;}}
  async function idle(ms){while(ms>0){await protect(Math.min(ms,10000));const step=Math.min(ms,10000,protectedUntil-(await game.observe()).game_ms);await game.act({keys:[],duration_ms:step});ms-=step;}}
  async function save(label){const state=await game.inspect();records.push({label,state});await writeFile(join(out,'encounter.json'),JSON.stringify(records));return state;}
  async function approach(point,range=2){
    for(let n=0;n<60;n++){
      await protect(250);const view=await game.observe(),dx=point.x-view.position.x,dy=point.y-view.position.y;if(Math.hypot(dx,dy)<range)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await game.act({keys,duration_ms:Math.max(50,Math.min(250,Math.round(Math.hypot(dx,dy)/10*1000)))});
    }
    throw Error('Native approach failed');
  }
  async function fire(id,ms=150){await protect(ms);const target=entity(await game.inspect(),id);assert(target,'Living target required');await approach(target.pos,2.5);await game.act({keys:[],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});}
  // Fire from the shore: the ship and its spirits come within wand range as they orbit, Follow and Charge.
  // Prepared +100 ATT keeps the 50000-HP ship within the receipt budget; it ends before each renewal gap.
  async function volley(id,ms){await protect(ms);const s=await game.inspect(),target=entity(s,id);assert(target,'Living target required');
    const visible=s.view.objects.find(o=>o.id===id&&o.hp>0);
    if(!visible||Math.hypot(visible.position.x-s.view.position.x,visible.position.y-s.view.position.y)>9.5){await idle(500);return;}
    await modifier(2,protectedUntil-1500);
    // Aim at the observed bearing from inside the playfield; the HUD can swallow clicks near the edge.
    const a=-Math.PI/4,dx=visible.position.x-s.view.position.x,dy=visible.position.y-s.view.position.y;
    const x=50*(dx*Math.cos(a)+dy*Math.sin(a)),y=50*(-dx*Math.sin(a)+dy*Math.cos(a)),scale=Math.max(1,Math.hypot(x,y)/90);
    await game.act({keys:[],duration_ms:ms,mouse:{x:300+x/scale,y:325+y/scale}});
  }
  const history=async()=>(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
  async function relocate(pos){
    // The client's 10s teleportTo cooldown runs on pause-adjusted page time, which tracks game time, not wall time.
    await idle(Math.max(0,lastTeleport+11000-(await game.observe()).game_ms));await protect();
    // The Anchor's PlaceMap stamps its NoWalk lake only once a player wakes it (VPS a1921956 stranded the scout inside it), so wake
    // the site first, then choose the nearest clear tile without entities on the map read afterwards.
    await cheat({kind:'teleport',position:pos},scout);await idle(2000);
    const now=await(await fetch(config.origin+'/dev/map?player='+(await game.observe()).session.player,{headers:{Authorization:'Bearer '+token}})).json(),blocked=new Set(now.blocked.map(p=>p.join(','))),occupied=new Set(now.entities.map(e=>[Math.floor(e.pos.x),Math.floor(e.pos.y)].join(',')));
    pos=Array.from({length:625},(_,i)=>({x:Math.floor(pos.x)+i%25-12+.5,y:Math.floor(pos.y)+Math.floor(i/25)-12+.5})).filter(p=>[-1,0,1].every(dx=>[-1,0,1].every(dy=>{const key=[Math.floor(p.x)+dx,Math.floor(p.y)+dy].join(',');return !blocked.has(key)&&!occupied.has(key);}))).sort((a,b)=>Math.hypot(a.x-pos.x,a.y-pos.y)-Math.hypot(b.x-pos.x,b.y-pos.y))[0];
    assert(pos,'No walkable scout placement within twelve tiles');
    await cheat({kind:'teleport',position:pos},scout);
    const client=config.native.find(c=>c.actor===game.actor),tabs=await(await fetch('http://127.0.0.1:'+client.port+'/json/list')).json(),tab=tabs.find(t=>t.id===client.target);assert(tab);
    const socket=new WebSocket(tab.webSocketDebuggerUrl);await new Promise((ok,fail)=>{socket.onopen=ok;socket.onerror=fail;});
    let serial=0;const pending=new Map();socket.onmessage=event=>{const m=JSON.parse(event.data),p=pending.get(m.id);if(p){pending.delete(m.id);m.error?p.reject(Error(JSON.stringify(m.error))):p.resolve(m.result);}};
    const {managedCDP}=await import(pathToFileURL(config.browserTools));
    const call=await managedCDP(client.port,client.target,process.env.PI_BROWSER_OWNER,(method,params)=>new Promise((resolve,reject)=>{const id=++serial;pending.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params}));}));
    const action=game.act({keys:[],duration_ms:4000});
    try{
      for(let n=0;(await game.observe()).paused;n++){assert(n<200,'Native action did not resume');await delay(20);}
      const enter=async()=>{await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13,text:'\r'});await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});};
      await enter();await call('Input.insertText',{text:'/teleport '+scoutName});await enter();
    }finally{socket.close();await action;}
    const view=await game.observe();assert(Math.hypot(view.position.x-pos.x,view.position.y-pos.y)<2,'Ordinary native /teleport must reach the prepared scout');lastTeleport=view.game_ms;
    // Park the scout at the spawn: its unarmored renewal gaps beside the lake let a Water Mine kill it (local baf65260).
    await cheat({kind:'teleport',position:home},scout);
  }
  let ship;
  for(const root of roots){
    await relocate(root.pos);await save('hero-'+root.id);
    for(let n=0;n<80;n++){
      const s=await game.inspect(),members=s.diagnostic.entities.filter(e=>(e.quest===root.id||e.id===root.id)&&[0x678,0x675,0x6c2,0x6c1,0x91f,0x920,0x91b,0x91c,0x91d,0x928,0x92d].includes(e.kind));
      if(!members.length)break;
      const victim=members.find(e=>e.kind===0x91d)??members.find(e=>!(e.condition&((1<<24)|(1<<23))));
      if(victim)await fire(victim.id,1500);else await idle(3000);
    }
    await idle(500);
    ship=(await history()).find(f=>f.message==='quest defeat summoned a Realm event'&&f.kind===0xe37);
    if(ship)break;
  }
  assert(ship,'Bounded ordinary hero defeats did not admit a Ghost Ship');
  game.check('qualifying native combat admits the reserved Ghost Ship',!!ship);
  const initialPosition={x:ship.x,y:ship.y};await relocate(initialPosition);
  let state=await save('ship-idle');const id=ship.entity,initial=entity(state,id);assert(initial?.kind===0xe37);
  const xp=state.view.character.experience;
  // Recovered Ghost Ship/Anchor states (actors.xml:11454,11741): the Anchor's spirits and the Ship's mines are owned by parent.
  const top=(s,actor)=>s.diagnostic.xml[actor]?.states[1]?.name,path=(s,actor)=>(s.diagnostic.xml[actor]?.states??[]).map(x=>x.name);
  const anchor=state.diagnostic.entities.find(e=>e.kind===0xe38&&e.quest===id);assert(anchor,'Encounter-owned Ghost Ship Anchor required');
  const family=s=>s.diagnostic.entities.filter(e=>e.id!==id&&(e.quest===id||e.parent===id||e.parent===anchor.id));
  const invulnerable=e=>!!(e.condition&(1<<24));
  // The Anchor enters MakeShip 2s after admission (actors.xml:11745-11754); its MakeMinions must not add a second ship.
  await idle(3000);state=await save('make-ship');
  game.check('summoned event keeps exactly one Ghost Ship',state.diagnostic.entities.filter(e=>e.kind===0xe37).length===1&&!!entity(state,id));
  game.check('new ship has selected 50000 HP and encounter-owned anchor',initial.hp===50000&&top(state,id)==='Harmless');
  await game.act({keys:[],duration_ms:800},{name:'ghost-ship-idle',when:()=>true});
  for(let n=0;n<40&&top(await game.inspect(),id)==='Harmless';n++)await volley(id,1000);
  state=await save('activated');game.check('native damage below 96% enters the shielded Change',top(state,id)==='Change'&&invulnerable(entity(state,id)));
  for(let n=0;n<20&&top(await game.inspect(),id)==='Change';n++)await idle(500);
  state=await save('battle');game.check('Change opens the battle with FireAtWill',top(state,id)==='FireAtWill');
  for(let n=0;n<20&&!family(await game.inspect()).some(e=>e.kind===0xe39&&e.parent===anchor.id);n++)await idle(1000);
  state=await save('skulls');const previousSpirits=new Set(family(state).filter(e=>e.kind===0xe39&&e.parent===anchor.id).map(e=>e.id)),spirit=family(state).find(e=>previousSpirits.has(e.id));assert(spirit,'Anchor-owned Vengeful Spirit required');
  game.check('ghostmode tag moves the Anchor into its Battle',path(state,anchor.id).includes('Battle'));
  for(let n=0;n<20&&entity(await game.inspect(),spirit.id);n++)await volley(spirit.id,750);
  state=await save('skull-defeated');game.check('summoned skull defeat grants no XP',!entity(state,spirit.id)&&state.view.character.experience===xp);
  await idle(5500);state=await save('skull-replacement');game.check('Anchor replenishes a new owned Vengeful Spirit',family(state).some(e=>e.kind===0xe39&&e.parent===anchor.id&&!previousSpirits.has(e.id)));
  const phases=new Set(),anchorPhases=new Set(),samples=[];
  // Bounded by the 256-receipt native budget (scripts/game.mjs), keeping a few for the closing checks; mines and spirits in the line of
  // fire absorb many shots (local 371645ca and 55f35f09 left ~8900 and ~7500 HP after 190 iterations), so spend every receipt.
  const receipts=async()=>(await readdir(join(config.state,'testing'))).filter(n=>n.endsWith('.client.json')).length;
  while(await receipts()<244){
    state=await game.inspect();const e=entity(state,id);if(!e)break;
    const now=path(state,id).slice(1).join('/');phases.add(top(state,id));for(const name of path(state,anchor.id))anchorPhases.add(name);
    const newShot=state.view.projectiles.some(p=>p.owner===id&&!samples.some(s=>s.view.projectiles.some(q=>q.owner===id&&q.speed===p.speed)));
    if(newShot||!samples.some(s=>path(s,id).slice(1).join('/')===now)){samples.push(state);await writeFile(join(out,'phases.json'),JSON.stringify(samples));}
    // Recovered shield phases still govern damage (actors.xml:11511–11572). The
    // 39838685 run reached 5,188 HP at the receipt budget; fire longer per legal input.
    if(invulnerable(e))await idle(2000);else await volley(id,5000);
    if(path(state,id)[1]==='MineDance'&&!records.some(r=>r.label==='mine-dance')){await save('mine-dance');await game.act({keys:[],duration_ms:800},{name:'ghost-ship-mine-dance',when:()=>true});}
  }
  state=await save('defeated');game.check('native combat defeats the ship',!entity(state,id));
  // MidBattle is the only deterministic hp gate after Change: Base checks it first below 60%, then PreMineDance leads to the 12s MineDance
  // (actors.xml:11488-11497). The 0.4s Base, 0.2s MidBattle and 1.2s PreMineDance transits fall between samples (local 64ee9178), so
  // MidBattle is read from the Anchor's storm phase below.
  for(const r of records)phases.add(top(r.state,id));
  game.check('native battle crosses the damage-gated Change and MineDance phases',['Harmless','Change','FireAtWill','MineDance'].every(name=>phases.has(name)));
  game.check('MidBattle orders the Anchor into its storm phase',anchorPhases.has('MidBattle'));
  game.check('ship moves during battle',samples.some(s=>{const e=entity(s,id);return e&&Math.hypot(e.pos.x-initial.pos.x,e.pos.y-initial.pos.y)>.2;}));
  game.check('native scene exposes both selected ship projectiles',[60,40].every(speed=>samples.some(s=>s.view.projectiles.some(p=>p.owner===id&&p.speed===speed))));
  const earned=state.view.character.experience;game.check('eligible finisher receives ship XP',earned>xp);
  await idle(4000);state=await save('settled');game.check('ship XP cannot be awarded twice',state.view.character.experience===earned);
  // The Anchor's Die state rolls Island (.25) and stamps davyisland.jm with four Davy Jones's Locker Portals it owns (actors.xml:11777-11788, geometry.xml).
  const lockers=family(state).filter(e=>e.kind===0x741);await writeFile(join(out,'portal-roll.json'),JSON.stringify({island:lockers.length>0,lockers}));
  game.check('noneExist Ghost Ship retires the Anchor and every owned battle actor',!entity(state,anchor.id)&&family(state).every(e=>e.kind===0x741));
  game.check('Anchor Die roll publishes either no Locker portal or the Island stamp\'s four',[0,4].includes(lockers.length));
  await game.act({keys:[],duration_ms:800},{name:'ghost-ship-defeated',when:()=>true});
}
