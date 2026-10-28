import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x30e,level:20,seed:860,world:'tomb-of-the-ancients',worldSeed:12,position:'spawn',equipment:[0xb08,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(observer){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(config.state,'control.token'),'utf8')).trim(),'Content-Type':'application/json'};
  let serial=0;
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:'tomb-switch-'+(++serial),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status!=='failed',JSON.stringify(s));return s;};
  const start=await observer.observe();
  const map=async()=>{const r=await fetch(config.origin+'/dev/map?player='+start.session.player,{headers,signal:AbortSignal.timeout(10000)});assert(r.ok);return r.json();};
  const layout=await map(),switches=layout.entities.filter(e=>e.kind===0xd37);assert(switches.length>=2,'Two generated trap rooms required');
  const {worldSeed,...spec}=fixture;
  const clients=[];
  for(const room of switches.slice(0,2)){
    const game=await observer.join({...spec,world:start.session.world,position:{x:room.pos.x+2,y:room.pos.y}});
    await api({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});
    clients.push({game,room,traps:layout.entities.filter(e=>e.kind===0xd38&&Math.hypot(e.pos.x-room.pos.x,e.pos.y-room.pos.y)<6)});
  }
  const [a,b]=clients,records=[];
  const save=async label=>{const state={label,a:await a.game.inspect(),b:await b.game.inspect(),arena:await observer.inspect(),map:await map()};records.push(state);await writeFile(join(out,'switches.json'),JSON.stringify(records));return state;};
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  async function walk(game,point){
    for(let n=0;n<25;n++){
      const v=await game.observe(),dx=point.x-v.position.x,dy=point.y-v.position.y;if(Math.hypot(dx,dy)<.25)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await game.act({keys,duration_ms:Math.max(50,Math.min(150,Math.round(Math.hypot(dx,dy)/(4+5.6*v.character.stats.speed/75)*1000)))});
    }
    throw Error('Native switch approach failed');
  }
  await a.game.act({keys:[],duration_ms:300},{name:'tomb-active-traps',when:()=>true});
  const initial=await save('initial');
  observer.check('two different generated rooms each have eight live traps',a.traps.length===8&&b.traps.length===8&&a.room.id!==b.room.id);
  await walk(a.game,a.room.pos);await a.game.act({keys:[],duration_ms:1400});
  const pressed=await save('partial-pressure');
  observer.check('native pressure starts without early activation',entity(pressed.a,a.room.id).tomb.press_since!==null&&entity(pressed.a,a.room.id).stage===0);
  await walk(a.game,{x:a.room.pos.x+2,y:a.room.pos.y});
  const interrupted=await save('interrupted');
  observer.check('leaving the switch resets uninterrupted pressure',entity(interrupted.a,a.room.id).tomb.press_since===null&&entity(interrupted.a,a.room.id).stage===0);
  await walk(b.game,b.room.pos);await b.game.act({keys:[],duration_ms:3300},{name:'tomb-second-room-disabled',when:()=>true});
  const local=await save('second-room-disabled');
  observer.check('another player disables only their own room',entity(local.b,b.room.id).stage===1&&entity(local.a,a.room.id).stage===0&&a.traps.every(t=>local.map.entities.some(e=>e.id===t.id&&e.kind===0xd38))&&b.traps.every(t=>!local.map.entities.some(e=>e.id===t.id)&&local.map.entities.some(e=>e.kind===0xd39&&e.pos.x===t.pos.x&&e.pos.y===t.pos.y)));
  await walk(a.game,a.room.pos);await a.game.act({keys:[],duration_ms:1400});
  const restarted=await save('restarted-pressure');
  observer.check('interrupted time is not accumulated across visits',entity(restarted.a,a.room.id).stage===0);
  await a.game.act({keys:[],duration_ms:2000},{name:'tomb-first-room-disabled',when:()=>true});
  const disabled=await save('both-disabled');
  observer.check('fresh uninterrupted pressure permanently disables all eight local traps',entity(disabled.a,a.room.id).stage===1&&a.traps.every(t=>!disabled.map.entities.some(e=>e.id===t.id)&&disabled.map.entities.some(e=>e.kind===0xd39&&e.pos.x===t.pos.x&&e.pos.y===t.pos.y)));
  observer.check('trap activation does not awaken statues or grant clear credit',disabled.map.entities.filter(e=>[0xd3d,0xd3e,0xd3f].includes(e.kind)).length===3&&disabled.arena.diagnostic.character.tombs===0&&disabled.a.diagnostic.character.tombs===0&&disabled.b.diagnostic.character.tombs===0);
  await a.game.act({keys:['Insert'],duration_ms:300});
  const escaped=await a.game.observe();observer.check('ordinary escape preserves the same living uncleared character',escaped.world==='Nexus'&&escaped.lifecycle.alive&&escaped.session.character===initial.a.view.session.character&&(await a.game.inspect()).diagnostic.character.tombs===0);
}
