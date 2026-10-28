import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {pathToFileURL} from 'node:url';
import {setTimeout as delay} from 'node:timers/promises';
const out=process.env.QA_RESULTS_DIR;
const spec={class:0x31d,level:20,seed:823,world:1,equipment:[0xb0b,0xb29,0xa7b,-1],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,0]};
let roots,blocked,scout,scoutName;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const spawned=await api({op:'spawn',spec:{...spec,guid:'ship-scout',position:{x,y},controller:'direct',profile:null}});scout=spawned.result.actor;
  const state=await api({op:'inspect',actor:scout});scoutName=state.diagnostic.character.name;assert(scoutName);
  const report=JSON.parse(await readFile(join(out,'native/result.json'),'utf8'));
  const token=(await readFile(join(out,'native/state/control.token'),'utf8')).trim();
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(response.ok);
  const map=await response.json();blocked=new Set(map.blocked.map(p=>p.join(',')));
  const kinds=[0x678,0x675,0x91f,0x91b,0x928];
  roots=map.entities.filter(e=>kinds.includes(e.kind)).sort((a,b)=>kinds.indexOf(a.kind)-kinds.indexOf(b.kind));assert(roots.length,'Natural Realm heroes required');
  await writeFile(join(out,'placement.json'),JSON.stringify({world,roots}));
  return {...spec,position:roots[0].pos};
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  let protectedUntil=0,scoutProtectedUntil=0,lastTeleport=0;const records=[];
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  async function cheat(change,actor=game.actor){const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor,change}})});const receipt=await r.json();assert(r.ok&&receipt.status==='succeeded',JSON.stringify(receipt));}
  async function protect(){const now=(await game.observe()).game_ms;if(now>=protectedUntil){await cheat({kind:'protection',condition:'invulnerable',duration_ms:60000});protectedUntil=now+60000;}if(now>=scoutProtectedUntil){await cheat({kind:'protection',condition:'invulnerable',duration_ms:60000},scout);scoutProtectedUntil=now+60000;}}
  async function idle(ms){while(ms>0){await protect();const step=Math.min(ms,10000,protectedUntil-(await game.observe()).game_ms);await game.act({keys:[],duration_ms:step});ms-=step;}}
  async function save(label){const state=await game.inspect();records.push({label,state});await writeFile(join(out,'encounter.json'),JSON.stringify(records));return state;}
  async function approach(point,range=2){
    for(let n=0;n<60;n++){
      await protect();const view=await game.observe(),dx=point.x-view.position.x,dy=point.y-view.position.y;if(Math.hypot(dx,dy)<range)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await game.act({keys,duration_ms:Math.max(50,Math.min(250,Math.round(Math.hypot(dx,dy)/10*1000)))});
    }
    throw Error('Native approach failed');
  }
  async function fire(id,ms=150){await protect();const target=entity(await game.inspect(),id);assert(target,'Living target required');await approach(target.pos,2.5);await game.act({keys:[],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});}
  const history=async()=>(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
  async function relocate(pos){
    assert(!blocked.has([Math.floor(pos.x),Math.floor(pos.y)].join(',')),'Scout placement must be walkable');
    await idle(Math.max(0,lastTeleport+11000-Date.now()));await protect();await cheat({kind:'teleport',position:pos},scout);
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
    const view=await game.observe();assert(Math.hypot(view.position.x-pos.x,view.position.y-pos.y)<2,'Ordinary native /teleport must reach the prepared scout');lastTeleport=Date.now();
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
  game.check('new ship has selected 50000 HP and encounter-owned anchor',initial.hp===50000&&state.diagnostic.entities.some(e=>e.kind===0xe38&&e.quest===id));
  await game.act({keys:[],duration_ms:800},{name:'ghost-ship-idle',when:()=>true});
  for(let n=0;n<20&&entity(await game.inspect(),id).stage===0;n++)await fire(id);
  state=await save('activated');game.check('native damage activates the shielded introduction',entity(state,id).stage===1&&!!(entity(state,id).condition&(1<<24)));
  await idle(3500);state=await save('battle');game.check('introduction opens the battle',entity(state,id).stage===3);
  await idle(8500);state=await save('skulls');const spirit=state.diagnostic.entities.find(e=>e.kind===0xe39&&e.quest===id);assert(spirit,'Owned Vengeful Spirit required');
  const xp=state.view.character.experience;
  for(let n=0;n<10&&entity(await game.inspect(),spirit.id);n++)await fire(spirit.id,750);
  state=await save('skull-defeated');game.check('summoned skull defeat grants no XP',!entity(state,spirit.id)&&state.view.character.experience===xp);
  await idle(5000);state=await save('skull-replacement');game.check('skull replenishment retains ownership and no-XP policy',state.diagnostic.entities.some(e=>e.kind===0xe39&&e.quest===id&&e.id!==spirit.id));
  const stages=new Set(),samples=[];
  for(let n=0;n<150;n++){
    state=await game.inspect();const e=entity(state,id);if(!e)break;stages.add(e.stage);
    if(!samples.some(s=>entity(s,id)?.stage===e.stage)){samples.push(state);await writeFile(join(out,'phases.json'),JSON.stringify(samples));}
    if(e.condition&(1<<24))await idle(1000);else await fire(id,750);
    if(e.stage===9&&!records.some(r=>r.label==='tempest')){await save('tempest');await game.act({keys:[],duration_ms:800},{name:'ghost-ship-tempest',when:()=>true});}
  }
  state=await save('defeated');game.check('native combat defeats the ship',!entity(state,id));
  game.check('native battle crosses every damage-dependent phase',[3,4,5,6,7,9,11].every(stage=>stages.has(stage)));
  game.check('ship moves during battle',samples.some(s=>{const e=entity(s,id);return e&&Math.hypot(e.pos.x-initial.pos.x,e.pos.y-initial.pos.y)>.2;}));
  game.check('native scene exposes both selected ship projectiles',[60,40].every(speed=>samples.some(s=>s.view.projectiles.some(p=>p.owner===id&&p.speed===speed))));
  const earned=state.view.character.experience;game.check('eligible finisher receives ship XP',earned>xp);
  await idle(4000);state=await save('settled');game.check('ship XP cannot be awarded twice',state.view.character.experience===earned);
  game.check('dependent battle actors retire after ship defeat',!state.diagnostic.entities.some(e=>e.quest===id&&[0xe38,0xe39,0xe3b,0xe3c,0xe4f].includes(e.kind)));
  const roll=(await history()).find(f=>f.message==='Ghost Ship Locker portal roll resolved'&&f.ship===id);assert(roll);
  await writeFile(join(out,'portal-roll.json'),JSON.stringify(roll));
  game.check('natural portal roll has one explicit recorded outcome',(await history()).filter(f=>f.message==='Ghost Ship Locker portal roll resolved'&&f.ship===id).length===1);
  await game.act({keys:[],duration_ms:800},{name:'ghost-ship-defeated',when:()=>true});
}
