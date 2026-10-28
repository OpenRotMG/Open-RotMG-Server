import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
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
  async function enter(from,name){
    await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
    for(let n=0;n<8&&(await game.observe()).session.world===from;n++)await game.act({keys:[],duration_ms:500});
    const s=await save(name);assert.notEqual(s.view.session.world,from);return s;
  }
  const initial=await save('nexus');
  await game.act({keys:['1'],duration_ms:400});
  const first=await enter(initial.view.session.world,'key-entry');
  game.check('ordinary Beachzone key is consumed once and admits the same Knight',first.view.map.name==='Beachzone'&&first.view.session.character===initial.view.session.character&&first.view.character.items[4]===-1&&first.view.character.inventory_revision===initial.view.character.inventory_revision+1);
  await game.act({keys:[],duration_ms:700},{name:'beach-entrance',when:()=>true});
  await walk(game,{x:44.5,y:65.5});
  const early=await enter(first.view.session.world,'early-exit');
  game.check('entrance escape returns living character to Nexus without clear credit',early.view.map.name==='Nexus'&&early.view.lifecycle.alive&&early.diagnostic.character.beaches===initial.diagnostic.character.beaches);
  await game.act({keys:['2'],duration_ms:400});
  const entry=await enter(early.view.session.world,'second-key-entry');
  await walk(game,{x:68.8,y:66.5});
  const arena=await save('arena'),boss=entities(arena).find(e=>e.kind===0xe56);assert(boss);
  game.check('ordinary entrance-to-stage walk reaches the only enemy',entities(arena).filter(e=>e.hp>0&&e.kind===0xe56).length===1&&arena.view.lifecycle.alive);
  const poses=new Set();
  for(let n=0;n<7;n++){
    await game.act({keys:[],duration_ms:500});
    const s=await save('dance-'+n),e=entities(s).find(e=>e.id===boss.id);assert(e);
    poses.add(e.extra_stats.find(([stat])=>stat===61)?.[1]);
    game.check('Party God remains stationary and never shoots '+n,distance(e.pos,boss.pos)<.01&&e.bullet===boss.bullet);
  }
  game.check('all three selected dance textures are published',poses.size===3&&[1,2,3].every(pose=>poses.has(pose)));
  await game.act({keys:[],duration_ms:700},{name:'party-god-dance',when:()=>true});
  await game.act({keys:[],duration_ms:700,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
  const damaged=await save('damaged');assert(entities(damaged).find(e=>e.id===boss.id).hp<30000);
  await game.act({keys:[],duration_ms:5500});
  const healed=await save('silent-recovery');
  game.check('un-stunned Party God silently recovers full health',entities(healed).find(e=>e.id===boss.id).hp===30000);
  const stunner=await game.join({...fixture,seed:866,world:entry.view.session.world,position:{x:69.5,y:66.5},equipment:[0xb0b,0xa08,0xafc,0xacb],backpack:Array(8).fill(0xa23)});
  const wizard=await game.join({class:0x30e,level:20,seed:867,world:entry.view.session.world,position:{x:70.5,y:68.5},equipment:[0xb08,0xa2e,0xb05,0xacb],backpack:Array(8).fill(0xa23),stats:[670,385,75,25,50,75,40,60]});
  const party=[game,stunner,wizard],target={entity:boss.id,camera_angle:-Math.PI/4,fire:true};
  let stunned=false,broken=false;
  for(let n=0;n<50;n++){
    const state=await save('combat-'+n),enemy=entities(state).find(e=>e.id===boss.id);if(!enemy)break;
    const inputs=[];
    for(const actor of party){
      const v=await actor.observe(),keys=n%2===0?[' ']:[];
      if(v.character.mp<100){const slot=v.character.items.findIndex((item,i)=>i>=4&&item===0xa23);if(slot>=4)keys.push(String(slot-3));}
      inputs.push({actor:actor.actor,input:{keys,duration_ms:1000,target}});
    }
    await game.act([inputs]);
    const e=entities(await save('after-volley-'+n)).find(e=>e.id===boss.id);
    if(e){stunned||=!!(e.condition&(1<<6));broken||=!!(e.condition&(1<<26));}
  }
  await game.act({keys:[],duration_ms:700},{name:'party-god-defeated',when:()=>true});
  const defeated=await save('defeated');
  game.check('ordinary cooperative sword, shield and spell combat defeats Party God',!entities(defeated).some(e=>e.id===boss.id)&&stunned&&broken);
  for(const actor of party){const s=await save('clear-credit',actor);game.check('participating character receives exactly one Beachzone clear '+actor.actor,s.diagnostic.character.beaches===1&&s.view.lifecycle.alive);}
  const exit=entities(defeated).find(e=>e.kind===0x704);assert(exit,'No committed Realm exit');
  const drinks=[0xc39,0xc3a,0xc3b,0xc3c];
  const bag=entities(defeated).find(e=>e.items.some(i=>drinks.includes(i))&&e.extra_stats.some(([stat,owner])=>stat===54&&owner===defeated.view.session.account));assert(bag,'No owned cocktail for first Knight in this seeded outcome');
  await walk(game,bag.pos);
  await game.act({keys:[],duration_ms:700},{name:'beach-reward',when:()=>true});
  const beforePickup=await save('before-pickup'),slot=bag.items.findIndex(i=>drinks.includes(i));assert(slot>=0);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},to:{x:634,y:428}}});
  const picked=await save('picked');
  game.check('ordinary loot drag commits an owned cocktail exactly once',drinks.includes(picked.view.character.items[4])&&picked.view.character.inventory_revision===beforePickup.view.character.inventory_revision+1);
  await walk(game,exit.pos);
  const returned=await enter(entry.view.session.world,'realm-exit');
  game.check('earned Realm exit preserves same living character, cocktail and once-only clear',returned.view.map.name==='Realm'&&returned.view.lifecycle.alive&&returned.view.session.character===initial.view.session.character&&returned.diagnostic.character.beaches===1&&returned.view.character.items[4]===picked.view.character.items[4]);
  await game.act({keys:['1'],duration_ms:500},{name:'cocktail-used',when:()=>true});
  const used=await save('cocktail-used');
  game.check('earned cocktail consumes once through ordinary item use',used.view.character.items[4]===-1&&used.view.character.inventory_revision===returned.view.character.inventory_revision+1&&used.diagnostic.character.beaches===1);
  const boosts={0xc39:[[0,50],[5,10]],0xc3a:[[0,50],[1,50]],0xc3b:[[2,10],[5,10]],0xc3c:[[3,10],[6,10]]}[picked.view.character.items[4]];
  game.check('earned cocktail applies its selected stat boosts and Drunk',boosts.every(([index,amount])=>used.view.character.stats[index]===returned.view.character.stats[index]+amount)&&!!(used.view.character.conditions&(1<<9)));
}
