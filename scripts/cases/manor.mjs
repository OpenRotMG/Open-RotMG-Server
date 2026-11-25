import assert from 'node:assert/strict';
import {nativeMap} from './native-map.mjs';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {walk as nativeWalk} from './native-route.mjs';

export const fixture={class:0x30e,level:20,seed:862,world:-2,position:'spawn',equipment:[0xb08,-1,0xb05,0xac5],backpack:[0xc23,-1,-1,-1,-1,-1,-1,-1],stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const out=name=>join(process.env.QA_RESULTS_DIR,name),samples=[],protections=[];
  const report=JSON.parse(await readFile(out('native/result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(out('native/state/control.token'),'utf8')).trim(),'Content-Type':'application/json'};
  const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y),entities=s=>s.diagnostic.entities;
  let map,reward=null,protectedUntil=0,armoredUntil=0;
  async function save(name){const s=await game.inspect();samples.push({name,state:s});await writeFile(out('journey.json'),JSON.stringify(samples));return s;}
  async function protect(){
    const response=await fetch(report.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
    protections.push(receipt);await writeFile(out('protection.json'),JSON.stringify(protections));
    protectedUntil=(await game.observe()).game_ms+60000;
  }
  async function refresh(){
    map=await nativeMap(report.origin,headers,(await game.inspect()).diagnostic.player);return map;
  }
  // Protection cannot be renewed early; a staggered DEF window covers each renewal gap beside Manor vampires.
  async function guard(){
    const v=await game.observe();if(!(v.character.conditions&(1<<23)))await protect();const left=protectedUntil-(await game.observe()).game_ms;
    if(left<=15000&&v.game_ms>=armoredUntil){const r=await fetch(report.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'stat_modifier',index:3,amount:100,duration_ms:60000}}})});assert(r.ok&&(await r.json()).status==='succeeded');armoredUntil=v.game_ms+60000;}
    return left;
  }
  async function step(input,capture){await guard();return game.act(input,capture);}
  // The generated Manor has single-gap doorways; the shared half-tile route keeps the two-tile collider clearance it needs.
  const walk=(goal,tolerance=.75)=>nativeWalk({client:{actor:game.actor,observe:()=>game.observe(),act:input=>step(input)},map:refresh,to:goal,options:()=>({reach:Math.max(tolerance,.3)}),record:r=>writeFile(out('manor-route.json'),JSON.stringify(r)),
    ready:guard});
  async function shoot(id,limit=25,duration_ms=1000){for(let n=0;n<limit;n++){const s=await game.inspect(),e=entities(s).find(e=>e.id===id);if(!e)return;await step({keys:[],duration_ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});}throw Error('Native shots did not defeat '+id);}
  async function enter(from,name){
    await step({keys:[],mouse:{x:700,y:575},duration_ms:700});
    for(let n=0;n<8&&(await game.observe()).session.world===from;n++)await step({keys:[],duration_ms:500});
    const s=await save(name);assert.notEqual(s.view.session.world,from);return s;
  }
  const initial=await save('nexus');
  await game.act({keys:['1'],duration_ms:500});
  const entry=await enter(initial.view.session.world,'key-entry');
  game.check('native Manor key is consumed and admits the same living Wizard',entry.view.map.name==='Manor of the Immortals'&&entry.view.character.items[4]===-1&&entry.view.session.character===initial.view.session.character&&entry.view.lifecycle.alive);
  await refresh();await writeFile(out('entry-map.json'),JSON.stringify(map));
  game.check('key-created Manor retains its ordinary vampire-room population',map.entities.some(e=>e.kind>=0x1727&&e.kind<=0x172c));
  await step({keys:[],duration_ms:700},{name:'manor-entrance',when:()=>true});
  const shelves=map.entities.filter(e=>e.kind===0x181a).sort((a,b)=>distance(a.pos,entry.view.position)-distance(b.pos,entry.view.position)),ordinary=shelves.find(e=>e.condition!==0);
  // Generated Manors hold several sealed rooms; open the secret shelves sealing a paired-coffin room.
  const coffins=map.entities.filter(e=>e.kind===0x1737),pair=coffins.find(a=>coffins.some(b=>a.id!==b.id&&distance(a.pos,b.pos)===4));assert(pair,'No generated paired-coffin room');
  const mate=coffins.find(e=>e.id!==pair.id&&distance(e.pos,pair.pos)===4),middle={x:(pair.pos.x+mate.pos.x)/2,y:(pair.pos.y+mate.pos.y)/2};
  assert(ordinary&&shelves.some(e=>e.condition===0),'Generated Manor must offer both shelf types for this case');
  await walk(ordinary.pos,4);
  await step({keys:[],duration_ms:500,target:{entity:ordinary.id,camera_angle:-Math.PI/4,fire:true}});
  await step({keys:[],duration_ms:700},{name:'ordinary-shelf',when:()=>true});
  await refresh();await writeFile(out('ordinary-shelf.json'),JSON.stringify({before:ordinary,after:map.entities.find(e=>e.id===ordinary.id)}));
  game.check('ordinary bookshelf survives native staff damage',map.entities.some(e=>e.id===ordinary.id&&e.hp===ordinary.hp));
  const doors=shelves.filter(e=>e.condition===0).sort((a,b)=>distance(a.pos,middle)-distance(b.pos,middle)).slice(0,3),passage={x:doors.reduce((sum,e)=>sum+e.pos.x,0)/doors.length,y:doors.reduce((sum,e)=>sum+e.pos.y,0)/doors.length};
  assert(doors.length===3&&doors.every(a=>doors.some(b=>b!==a&&distance(a.pos,b.pos)===1)),'Selected secret passage must have three adjacent shelves');
  await walk(passage,3);
  await step({keys:[],duration_ms:700},{name:'secret-shelf-before',when:()=>true});
  for(const door of doors){await walk(door.pos,3);await shoot(door.id);}
  await refresh();game.check('native staff destroys secret shelves and opens their blocked passage',doors.every(door=>!map.entities.some(e=>e.id===door.id)&&!map.blocked.some(([x,y])=>x===Math.floor(door.pos.x)&&y===Math.floor(door.pos.y))));
  await walk(passage,.6);
  await step({keys:[],duration_ms:700},{name:'secret-shelf-after',when:()=>true});
  const outcomes=[];
  for(const coffin of [pair,mate]){
    await refresh();await walk(coffin.pos,4);
    const before=await save('coffin-'+coffin.id+'-before'),known=new Set(entities(before).map(e=>e.id));
    if(coffin.id===pair.id)game.check('opened secret passage admits native walking to its paired coffins',distance(before.view.position,pair.pos)<=4.5&&entities(before).some(e=>e.id===mate.id));
    for(let n=0;n<20;n++){
      const e=entities(await game.inspect()).find(e=>e.id===coffin.id);if(!e||e.hp<20000)break;
      await step({keys:[],duration_ms:150,target:{entity:coffin.id,camera_angle:-Math.PI/4,fire:true}});
    }
    await step({keys:[],duration_ms:150});
    const opened=await game.inspect(),impostor=entities(opened).find(e=>e.kind===0x1722&&!known.has(e.id)&&distance(e.pos,coffin.pos)<.1);
    assert(impostor||entities(opened).find(e=>e.id===coffin.id)?.hp<20000,'Native shot must resolve the coffin opening');
    if(impostor){
      await shoot(impostor.id,100,150);const dead=await save('impostor-'+coffin.id);
      const brood=entities(dead).filter(e=>e.kind===0x1738&&!known.has(e.id));
      game.check('native impostor releases Lil Feratu without private loot or Manor credit',brood.length>=1&&brood.length<=4&&!entities(dead).some(e=>e.items?.some(i=>i>=0)&&distance(e.pos,coffin.pos)<1)&&dead.diagnostic.character.manors===before.diagnostic.character.manors);
      outcomes.push({id:coffin.id,impostor:true,brood:brood.length});
    }else{
      for(let n=0;n<30;n++){
        const s=await game.inspect(),e=entities(s).find(e=>e.id===coffin.id);if(!e)break;
        await step({keys:[],duration_ms:1000,target:{entity:coffin.id,camera_angle:-Math.PI/4,fire:true}});
        const after=await game.inspect();
        if(e.hp>15000&&entities(after).find(x=>x.id===coffin.id)?.hp<=15000)await save('coffin-first-wave');
        if(e.hp>8000&&entities(after).find(x=>x.id===coffin.id)?.hp<=8000)await save('coffin-second-wave');
      }
      const dead=await save('genuine-'+coffin.id);assert(!entities(dead).some(e=>e.id===coffin.id));
      const bags=entities(dead).filter(e=>e.extra_stats.some(([stat,owner])=>stat===54&&owner===dead.view.session.account)&&distance(e.pos,coffin.pos)<1);
      game.check('genuine coffin defeat does not grant Manor clear credit',dead.diagnostic.character.manors===before.diagnostic.character.manors);
      outcomes.push({id:coffin.id,impostor:false,bags:bags.map(e=>e.items)});
      if(bags.length&&reward===null){
        await refresh();await walk(bags[0].pos);await step({keys:[],duration_ms:700},{name:'coffin-private-reward',when:()=>true});
        const beforePickup=await game.observe();
        await step({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
        reward=await save('coffin-reward-collected');
        game.check('ordinary loot drag commits genuine private coffin reward once',reward.view.character.items[4]>=0&&reward.view.character.inventory_revision===beforePickup.character.inventory_revision+1);
      }
    }
    if(outcomes.some(o=>o.impostor)&&outcomes.some(o=>!o.impostor&&o.bags.length))break;
  }
  await writeFile(out('coffin-outcomes.json'),JSON.stringify(outcomes));
  await refresh();const boss=map.entities.find(e=>e.kind===0x1720);assert(boss);
  await walk(boss.pos,5);const arena=await save('ruthven-arrival');
  game.check('native walk traverses populated connected rooms to Ruthven',arena.view.lifecycle.alive&&arena.diagnostic.character.shots>entry.diagnostic.character.shots);
  await step({keys:[],duration_ms:700},{name:'ruthven-before',when:()=>true});
  for(let n=0;n<84;n++){
    const s=await game.inspect(),e=entities(s).find(e=>e.id===boss.id);if(!e)break;
    if(distance(e.pos,s.view.position)>6){await walk(e.pos,4);continue;}
    const left=await guard();
    // A full firing hold can overrun the protection deadline in a room full of active coffins.
    if(left<=200){await game.act({keys:[],duration_ms:Math.max(50,left+50)});continue;}
    const live=(await game.inspect()).view.objects.some(o=>o.id===boss.id&&o.hp>0);
    await game.act({keys:[],duration_ms:Math.min(1500,left-150),target:live?{entity:boss.id,camera_angle:-Math.PI/4,fire:true}:undefined});
  }
  game.check('native staff defeats Ruthven',!entities(await save('ruthven-defeated')).some(e=>e.id===boss.id));
  // The recovered Vampire Summoner turns into the Realm Portal once no Lord Ruthven exists (manorOfTheImmortals.xml:939-957).
  let exit;for(let n=0;n<10&&!exit;n++){await refresh();exit=map.entities.find(e=>e.kind===0x704);if(!exit)await step({keys:[],duration_ms:500});}
  assert(exit,'Vampire Summoner must publish the earned Realm Portal');
  const defeated=await save('manor-cleared');
  game.check('Ruthven defeat awards exactly one Manor completion',defeated.diagnostic.character.manors===entry.diagnostic.character.manors+1);
  await step({keys:[],duration_ms:700},{name:'ruthven-after',when:()=>true});
  await walk(exit.pos);
  const returned=await enter(entry.view.session.world,'boss-exit');
  game.check('earned Realm Portal returns the same living Wizard to the Realm with durable clear credit',returned.view.map.name==='Realm'&&returned.view.lifecycle.alive&&returned.view.session.character===initial.view.session.character&&returned.diagnostic.character.manors===defeated.diagnostic.character.manors);
  if(reward)game.check('collected private coffin reward persists through the earned exit',returned.view.character.items[4]===reward.view.character.items[4]);
  await step({keys:[],duration_ms:700},{name:'manor-return',when:()=>true});
}
