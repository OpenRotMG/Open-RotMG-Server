import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x31e,level:20,seed:865,world:-2,position:'spawn',equipment:[0xb0b,0xc0f,0xafc,0xacb],backpack:[0xc3d,0xc3d,0xa23,0xa23,0xa23,0xa23,0xa23,0xa23],stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const output=name=>join(process.env.QA_RESULTS_DIR,name),samples=[];
  const entities=s=>s.diagnostic.entities,distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
  async function save(name,actor=game){const state=await actor.inspect();samples.push({name,actor:actor.actor,state});await writeFile(output('journey.json'),JSON.stringify(samples));return state;}
  async function walk(actor,point,tolerance=.65){
    for(let n=0;n<100;n++){
      const v=await actor.observe(),dx=point.x-v.position.x,dy=point.y-v.position.y;
      if(Math.hypot(dx,dy)<tolerance)return;
      const keys=Math.abs(dx)>Math.abs(dy)?(dx>0?['d','s']:['a','w']):(dy>0?['a','s']:['d','w']);
      await actor.act({keys,duration_ms:Math.max(80,Math.min(350,Math.round(Math.max(Math.abs(dx),Math.abs(dy))/(4+5.6*v.character.stats.speed/75)*1000)))});
    }
    throw Error('Native walk did not reach '+JSON.stringify(point));
  }
  async function enter(from,name,actor=game){
    await actor.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
    for(let n=0;n<8&&(await actor.observe()).session.world===from;n++)await actor.act({keys:[],duration_ms:500});
    const s=await save(name,actor);assert.notEqual(s.view.session.world,from);return s;
  }
  const initial=await save('nexus');
  await game.act({keys:['1'],duration_ms:400});
  const first=await enter(initial.view.session.world,'key-entry');
  game.check('ordinary Beachzone key is consumed once and admits the same Knight',first.view.map.name==='Beachzone'&&first.view.session.character===initial.view.session.character&&first.view.character.items[4]===-1&&first.view.character.inventory_revision===initial.view.character.inventory_revision+1);
  await game.act({keys:[],duration_ms:700},{name:'beach-entrance',when:()=>true});
  await walk(game,{x:44.5,y:65.5});
  const early=await enter(first.view.session.world,'early-exit');
  game.check('entrance escape returns living character to Nexus without clear credit',early.view.map.name==='Nexus'&&early.view.lifecycle.alive&&early.diagnostic.character.beaches===initial.diagnostic.character.beaches);
  // As for the first supplied key, allow the relinked client to paint the new nearby portal before clicking Enter.
  await game.act({keys:['2'],duration_ms:700});
  const opened=await save('second-key-opened');
  const portal=opened.view.objects.find(o=>o.kind===0x742&&!early.view.objects.some(p=>p.id===o.id));
  assert(portal&&opened.view.character.items[5]===-1&&opened.view.character.inventory_revision===early.view.character.inventory_revision+1,'Second Beachzone key must publish a fresh portal and commit once');
  await game.act({keys:[],duration_ms:700},{name:'beach-second-portal',when:v=>v.nearby.some(o=>o.id===portal.id)});
  const entry=await enter(early.view.session.world,'second-key-entry');
  assert.notEqual(entry.view.session.world,first.view.session.world,'Fresh key must admit a distinct Beachzone');
  await walk(game,{x:68.8,y:66.5});
  const arena=await save('arena'),boss=entities(arena).find(e=>e.kind===0xe56);assert(boss);
  game.check('ordinary entrance-to-stage walk reaches the only enemy',entities(arena).filter(e=>e.hp>0&&e.kind===0xe56).length===1&&arena.view.lifecycle.alive);
  const poses=new Set();
  for(let n=0;n<7;n++){
    await game.act({keys:[],duration_ms:350});
    const s=await save('dance-'+n),e=entities(s).find(e=>e.id===boss.id);assert(e);
    poses.add(e.extra_stats.find(([stat])=>stat===61)?.[1]);
    game.check('Party God remains stationary and never shoots '+n,distance(e.pos,boss.pos)<.01&&e.bullet===boss.bullet);
  }
  // Recovered Base cycles default/3; only damaged Say publishes 1 and 2 (ghostShip.xml:66-96; actors.xml:12399-12424).
  game.check('full-health dance alternates only default and texture 3',poses.size===2&&poses.has(3)&&(poses.has(undefined)||poses.has(0)));
  await game.act({keys:[],duration_ms:700},{name:'party-god-dance',when:()=>true});
  const alternate=s=>entities(s).find(e=>e.id===boss.id).extra_stats.find(([stat])=>stat===61)?.[1];
  const inSay=s=>s.diagnostic.xml[boss.id].states.some(state=>state.name==='Say');
  const damagedPoses=new Set();
  for(let n=0;n<7&&!([1,2].every(pose=>damagedPoses.has(pose)));n++){
    let pending,pollError;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>{if(inSay(s))damagedPoses.add(alternate(s));}).catch(e=>{pollError=e;}).finally(()=>pending=null);},50);
    try{await game.act({keys:[],duration_ms:900,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});}finally{clearInterval(timer);if(pending)await pending;}
    if(pollError)throw pollError;
  }
  game.check('only damage-triggered Say publishes alternate textures 1 and 2',[1,2].every(pose=>damagedPoses.has(pose)));
  await game.act({keys:[],duration_ms:2500});
  const healed=await save('silent-recovery');
  game.check('un-stunned Party God recovers full health',entities(healed).find(e=>e.id===boss.id).hp===30000);
  const stunner=await game.join({...fixture,seed:866,world:entry.view.session.world,position:{x:69.5,y:66.5},equipment:[0xb0b,0xa08,0xafc,0xacb],backpack:[-1,...Array(7).fill(0xa23)]});
  const wizard=await game.join({class:0x30e,level:20,seed:867,world:entry.view.session.world,position:{x:70.5,y:68.5},equipment:[0xb08,0xa2e,0xb05,0xacb],backpack:[-1,...Array(7).fill(0xa23)],stats:[670,385,75,25,50,75,40,60]});
  const party=[game,stunner,wizard],target={entity:boss.id,camera_angle:-Math.PI/4,fire:true};
  // Heal has no stun check, and hideVisual suppresses its line (actors.xml:12431; behavior.cc:1255-1288).
  const observations=[];
  for(let n=0;n<6;n++){
    let pending,pollError;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>{
      const e=entities(s).find(e=>e.id===boss.id),at=s.diagnostic.time_ms;
      observations.push({at,hp:e.hp,stunned:e.statuses['7']?.[1]>at,broken:e.statuses['27']?.[1]>at});
    }).catch(e=>{pollError=e;}).finally(()=>pending=null);},50);
    const volley=party.map(actor=>({actor:actor.actor,input:{keys:n%2===0?[' ']:[],duration_ms:900,target}}));
    try{await game.act([volley]);}finally{clearInterval(timer);if(pending)await pending;}
    if(pollError)throw pollError;
  }
  await writeFile(output('party-heal.json'),JSON.stringify(observations));
  game.check('cooperative native attacks apply Stunned and Armor Broken together',observations.some(s=>s.stunned&&s.broken&&s.hp<30000));
  game.check('rapid self-healing continues during both conditions',observations.some((a,i)=>a.stunned&&a.broken&&observations.slice(i+1).some(b=>b.at>a.at&&b.at-a.at<=400&&b.stunned&&b.broken&&b.hp>a.hp)));
  const end=await save('party-healed');
  game.check('cooperative attack does not imply a completion or reward',end.diagnostic.character.beaches===0&&entities(end).some(e=>e.id===boss.id&&e.hp>0));
  // Earned kill, Realm exit and private cocktails are separate intent: this group cannot outrun the recovered Heal.
}
