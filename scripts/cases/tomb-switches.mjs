import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

// Tomb Bomb Trap Switch (actors.xml:7565-7603): ObjectToss six traps; a player within one tile for WaitAgain0.2+Wait0.6+three1.2 ticks reaches Done, whose Order Metamorphoses its listening traps.
export const fixture={class:0x30e,level:20,seed:860,world:'tomb-of-the-ancients',worldSeed:12,position:'spawn',equipment:[0xb08,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(observer){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(config.state,'control.token'),'utf8')).trim(),'Content-Type':'application/json'};
  let serial=0;
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:'tomb-switch-'+(++serial),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status!=='failed',JSON.stringify(s));return s;};
  const start=await observer.observe();
  const map=async()=>{const r=await fetch(config.origin+'/dev/map?player='+start.session.player,{headers,signal:AbortSignal.timeout(10000)});assert(r.ok);return r.json();};
  const layout=await map(),switches=layout.entities.filter(e=>e.kind===0xd37);assert(switches.length>=2,'Two generated trap rooms required');
  assert(!layout.entities.some(e=>e.kind===0xd38),'Traps must come from switch tosses, not the map');
  const {worldSeed,...spec}=fixture;
  const near=(m,room,kind)=>m.entities.filter(e=>e.kind===kind&&Math.hypot(e.pos.x-room.pos.x,e.pos.y-room.pos.y)<10);
  const clients=[];
  for(const room of switches.slice(0,2)){
    const game=await observer.join({...spec,world:start.session.world,position:{x:room.pos.x+2,y:room.pos.y}});
    await api({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});
    await game.act({keys:[],duration_ms:2500});
    clients.push({game,room,traps:near(await map(),room,0xd38)});
  }
  const [a,b]=clients,records=[];
  const save=async label=>{const state={label,a:await a.game.inspect(),b:await b.game.inspect(),arena:await observer.inspect(),map:await map()};records.push(state);await writeFile(join(out,'switches.json'),JSON.stringify(records));return state;};
  const phase=(s,id)=>s.diagnostic.xml[id]?.states.at(-1)?.name;
  const off=(m,c)=>c.traps.every(t=>!m.entities.some(e=>e.id===t.id)&&near(m,c.room,0xd39).some(e=>e.pos.x===t.pos.x&&e.pos.y===t.pos.y));
  const live=(m,c)=>c.traps.every(t=>m.entities.some(e=>e.id===t.id&&e.kind===0xd38));
  async function walk(game,point,range=.25){
    for(let n=0;n<25;n++){
      const v=await game.observe(),dx=point.x-v.position.x,dy=point.y-v.position.y;if(Math.hypot(dx,dy)<range)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await game.act({keys,duration_ms:Math.max(50,Math.min(150,Math.round(Math.hypot(dx,dy)/(4+5.6*v.character.stats.speed/75)*1000)))});
    }
    throw Error('Native switch approach failed');
  }
  await a.game.act({keys:[],duration_ms:300},{name:'tomb-active-traps',when:()=>true});
  const initial=await save('initial');
  observer.check('each generated switch tosses its own one to six live traps',[a,b].every(c=>c.traps.length>=1&&c.traps.length<=6&&c.traps.every(t=>t.parent===c.room.id))&&a.room.id!==b.room.id);
  // Prepared acts can advance more game time than requested under load, so poll the switch state instead of timing it.
  async function hold(c,want){for(let n=0;n<60;n++){const s=await c.game.inspect(),p=phase(s,c.room.id);if(want.test(p))return p;assert(p!=='Done'||want.test('Done'),'Switch finished before '+want);await c.game.act({keys:[],duration_ms:150});}throw Error('Switch never reached '+want);}
  await walk(a.game,a.room.pos,.8);await hold(a,/^Tick1$/);
  const pressed=await save('partial-pressure');
  observer.check('native pressure starts the switch count without early activation',/^Tick[123]$/.test(phase(pressed.a,a.room.id))&&live(pressed.map,a));
  // One ordinary eastward step leaves the one-tile radius before Tick1's 1.2s elapses.
  await a.game.act({keys:['d','s'],duration_ms:400});await hold(a,/^Wait(Again)?$/);
  const interrupted=await save('interrupted');
  observer.check('leaving the switch abandons the count',/^Wait(Again)?$/.test(phase(interrupted.a,a.room.id))&&live(interrupted.map,a));
  await walk(b.game,b.room.pos,.8);await hold(b,/^Done$/);
  await b.game.act({keys:[],duration_ms:300},{name:'tomb-second-room-disabled',when:()=>true});
  const local=await save('second-room-disabled');
  observer.check('another player disables only their own room',phase(local.b,b.room.id)==='Done'&&phase(local.a,a.room.id)!=='Done'&&live(local.map,a)&&off(local.map,b));
  await walk(a.game,a.room.pos,.8);await hold(a,/^Done$/);
  await a.game.act({keys:[],duration_ms:300},{name:'tomb-first-room-disabled',when:()=>true});
  const disabled=await save('both-disabled');
  observer.check('a fresh uninterrupted count permanently disables every local trap',phase(disabled.a,a.room.id)==='Done'&&off(disabled.map,a));
  observer.check('trap activation does not awaken statues or grant clear credit',disabled.map.entities.filter(e=>[0xd3d,0xd3e,0xd3f].includes(e.kind)).length===3&&disabled.arena.diagnostic.character.tombs===0&&disabled.a.diagnostic.character.tombs===0&&disabled.b.diagnostic.character.tombs===0);
  await a.game.act({keys:['Insert'],duration_ms:300});
  const escaped=await a.game.observe();observer.check('ordinary escape preserves the same living uncleared character',escaped.world==='Nexus'&&escaped.lifecycle.alive&&escaped.session.character===initial.a.view.session.character&&(await a.game.inspect()).diagnostic.character.tombs===0);
}
