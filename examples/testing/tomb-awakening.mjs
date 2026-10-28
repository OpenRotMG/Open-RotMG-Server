import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x323,level:20,seed:860,world:'tomb-of-the-ancients',worldSeed:12,position:'spawn',equipment:[0xb0a,0xb2d,0xb05,0xacd],backpack:Array(8).fill(-1),stats:[670,385,60,25,60,50,40,75]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(config.state,'control.token'),'utf8')).trim(),'Content-Type':'application/json'};
  let serial=0;
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:'tomb-awakening-'+(++serial),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status!=='failed',JSON.stringify(s));return s;};
  const start=await game.observe();
  const map=async()=>{const r=await fetch(config.origin+'/dev/map?player='+start.session.player,{headers,signal:AbortSignal.timeout(10000)});assert(r.ok);return r.json();};
  const layout=await map(),objectives=layout.entities.filter(e=>e.kind===0xd29),statues=layout.entities.filter(e=>[0xd3d,0xd3e,0xd3f].includes(e.kind));assert(objectives.length>=3&&statues.length===3);
  const records=[],support=[],expiry=new Map(),humans=[game.actor];let protection=0;
  const save=async label=>{const s={label,state:await game.inspect(),map:await map()};records.push(s);await writeFile(join(out,'awakening.json'),JSON.stringify(records));return s;};
  async function protect(){
    const now=(await game.observe()).game_ms;
    if(now<protection)return;
    for(const actor of [...humans,...support]){
      if((expiry.get(actor)??0)>now)continue;
      await api({op:'cheat',actor,change:{kind:'protection',condition:humans.includes(actor)?'invulnerable':'invincible',duration_ms:60000}});
      if(!humans.includes(actor))await api({op:'cheat',actor,change:{kind:'stat_modifier',index:2,amount:100,duration_ms:60000}});
      expiry.set(actor,now+60000);
    }
    protection=Math.min(...expiry.values());
  }
  async function advance(ms,capture){
    while(ms>0){await protect();const step=Math.min(ms,10000,protection-(await game.observe()).game_ms);await game.act({keys:[],duration_ms:step},capture);capture=undefined;ms-=step;}
  }
  const wizard={class:0x30e,level:20,seed:860,world:start.session.world,equipment:[0xb08,0xa2e,0xb05,0xacd],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60],controller:'game_bot',profile:'Bugmaster'};
  async function squad(objective){
    for(let n=0;n<6;n++){
      const actor=(await api({op:'spawn',spec:{...wizard,guid:'tomb-squad-'+objective.id+'-'+n,position:{x:objective.pos.x-1+(n%3),y:objective.pos.y-2+Math.floor(n/3)}}})).result.actor;
      support.push(actor);
      await api({op:'cheat',actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});
      await api({op:'cheat',actor,change:{kind:'stat_modifier',index:2,amount:100,duration_ms:60000}});
      expiry.set(actor,(await game.observe()).game_ms+60000);
    }
  }
  const combat=()=>advance(2000);
  for(const objective of objectives.slice(0,-1))await squad(objective);
  const final=objectives.at(-1);
  for(let n=0;n<30;n++){
    await combat();
    const m=await map();if(m.entities.filter(e=>e.kind===0xd29).length===1)break;
  }
  const gated=await save('last-objective-gate');
  game.check('all but the last objective can die without awakening any statue',gated.map.entities.filter(e=>e.kind===0xd29).length===1&&gated.map.entities.some(e=>e.id===final.id)&&statues.every(e=>gated.map.entities.some(x=>x.id===e.id))&&!gated.map.entities.some(e=>[0xd26,0xd27,0xd28].includes(e.kind)));
  game.check('optional treasure remains outside the objective gate',gated.map.entities.some(e=>e.kind===0xd2c));
  await squad(final);
  for(let n=0;n<30;n++){await combat();if(!(await map()).entities.some(e=>e.kind===0xd29))break;}
  const awakened=await save('awakened'),bosses=awakened.map.entities.filter(e=>[0xd26,0xd27,0xd28].includes(e.kind));
  game.check('last real objective death replaces all three statues exactly once',!awakened.map.entities.some(e=>e.kind===0xd29)&&bosses.length===3&&new Set(bosses.map(e=>e.kind)).size===3&&statues.every(e=>!awakened.map.entities.some(x=>x.id===e.id)));
  game.check('awakening does not grant completion credit',awakened.state.diagnostic.character.tombs===0);
  const {worldSeed,...spec}=fixture;
  const late=await game.join({...spec,world:start.session.world,position:'spawn'});
  await api({op:'cheat',actor:late.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});
  humans.push(late.actor);expiry.set(late.actor,(await game.observe()).game_ms+60000);
  await advance(3000);
  await advance(500,{name:'tomb-late-join-awakened',when:()=>true});
  const joined=await late.inspect();await writeFile(join(out,'late-join.json'),JSON.stringify(joined));
  game.check('late native join sees the same three living boss identities without statues',bosses.every(e=>joined.diagnostic.entities.some(x=>x.id===e.id&&x.kind===e.kind))&&!joined.diagnostic.entities.some(e=>[0xd3d,0xd3e,0xd3f].includes(e.kind))&&joined.diagnostic.character.tombs===0);
  const repeated=await save('after-late-join');game.check('late admission does not duplicate or replace awakened bosses',repeated.map.entities.filter(e=>[0xd26,0xd27,0xd28].includes(e.kind)).length===3&&bosses.every(e=>repeated.map.entities.some(x=>x.id===e.id)));
  const nut=bosses.find(e=>e.kind===0xd26),phases=[];let artifact,modifier=0;
  for(let n=0;n<80;n++){
    await protect();const now=(await game.observe()).game_ms;
    if(now>=modifier){await api({op:'cheat',actor:game.actor,change:{kind:'stat_modifier',index:2,amount:100,duration_ms:60000}});modifier=now+60000;}
    const s=await game.inspect();phases.push(s);
    artifact=s.diagnostic.entities.filter(e=>e.kind>=0xd1c&&e.kind<=0xd1e).sort((a,b)=>Math.hypot(a.pos.x-s.view.position.x,a.pos.y-s.view.position.y)-Math.hypot(b.pos.x-s.view.position.x,b.pos.y-s.view.position.y))[0];
    if(artifact)break;
    assert(s.diagnostic.entities.some(e=>e.id===nut.id),'Nut disappeared before artifact acceptance');
    await game.act({keys:[],duration_ms:Math.min(1500,protection-now),target:{entity:nut.id,camera_angle:-Math.PI/4,fire:true}});
  }
  await writeFile(join(out,'nut-phases.json'),JSON.stringify(phases));assert(artifact,'Native combat did not reach Nut artifacts within the bound');
  await advance(1700);await protect();
  const remaining=protection-(await game.observe()).game_ms;
  if(remaining<500){await advance(remaining);await protect();}
  const drained=await game.inspect();artifact=drained.diagnostic.entities.filter(e=>e.kind>=0xd1c&&e.kind<=0xd1e).sort((a,b)=>Math.hypot(a.pos.x-drained.view.position.x,a.pos.y-drained.view.position.y)-Math.hypot(b.pos.x-drained.view.position.x,b.pos.y-drained.view.position.y))[0];assert(artifact,'A living artifact is required after friendly shots expire');
  const orbAt=(await game.observe()).game_ms;
  await game.act({keys:[' '],duration_ms:500,target:{entity:artifact.id,camera_angle:-Math.PI/4,fire:false}},{name:'tomb-artifact-stasis',when:v=>v.game_ms>=orbAt+300});
  const frozen=await save('artifact-stasis'),actual=frozen.state.diagnostic.entities.find(e=>e.id===artifact.id);
  game.check('native Orb Stasises a naturally spawned Tomb artifact without damage',actual?.statuses['22']&&actual.hp===artifact.hp);
  game.check('artifact combat does not grant premature three-boss completion',frozen.state.diagnostic.character.tombs===0&&frozen.map.entities.filter(e=>[0xd26,0xd27,0xd28].includes(e.kind)).length===3);
  await game.act({keys:['Insert'],duration_ms:300});
  const escaped=await game.observe();game.check('uncleared escape retains the same living character and zero completion',escaped.world==='Nexus'&&escaped.lifecycle.alive&&escaped.session.character===start.session.character&&(await game.inspect()).diagnostic.character.tombs===0);
}
