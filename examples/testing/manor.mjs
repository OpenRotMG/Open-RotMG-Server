import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

export const fixture={class:0x30e,level:20,seed:862,world:-2,position:'spawn',equipment:[0xb08,-1,0xb05,0xac5],backpack:[0xc23,-1,-1,-1,-1,-1,-1,-1],stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const out=name=>join(process.env.QA_RESULTS_DIR,name),samples=[],protections=[];
  const report=JSON.parse(await readFile(out('native/result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(out('native/state/control.token'),'utf8')).trim(),'Content-Type':'application/json'};
  const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y),entities=s=>s.diagnostic.entities;
  let protectionUntil=0,map;
  async function save(name){const s=await game.inspect();samples.push({name,state:s});await writeFile(out('journey.json'),JSON.stringify(samples));return s;}
  async function protect(){
    const response=await fetch(report.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:20000}}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
    protectionUntil=(await game.observe()).game_ms+20000;protections.push(receipt);await writeFile(out('protection.json'),JSON.stringify(protections));
  }
  async function refresh(){
    const s=await game.inspect(),response=await fetch(report.origin+'/dev/map?player='+s.diagnostic.player,{headers});assert(response.ok);
    map=await response.json();return map;
  }
  async function step(input,capture){if((await game.observe()).game_ms>=protectionUntil)await protect();return game.act(input,capture);}
  function route(start,goal,tolerance){
    const width=map.width,blocked=new Set(map.blocked.map(([x,y])=>y*width+x)),first=Math.floor(start.y)*width+Math.floor(start.x),previous=new Map([[first,null]]),queue=[first];
    for(let n=0;n<queue.length;n++){
      const i=queue[n],p={x:i%width+.5,y:Math.floor(i/width)+.5};
      if(distance(p,goal)<=tolerance){const path=[];for(let at=i;at!==null;at=previous.get(at))path.push({x:at%width+.5,y:Math.floor(at/width)+.5});return path.reverse();}
      for(const j of [i-1,i+1,i-width,i+width])if(j>=0&&j<width*map.height&&!blocked.has(j)&&!previous.has(j)){previous.set(j,i);queue.push(j);}
    }
    throw Error('No walkable route to '+JSON.stringify(goal));
  }
  async function walk(goal,tolerance=.75){
    const path=route((await game.observe()).position,goal,tolerance),turns=path.filter((p,i)=>i===path.length-1||i>0&&(p.x-path[i-1].x!==path[i+1].x-p.x||p.y-path[i-1].y!==path[i+1].y-p.y));
    for(const point of turns)for(let n=0;n<80;n++){
      const v=await game.observe(),dx=point.x-v.position.x,dy=point.y-v.position.y,d=distance(point,v.position);if(d<.3)break;
      assert(n<79,'Native walk stalled at '+JSON.stringify({point,position:v.position}));
      const keys=Math.abs(dx)>Math.abs(dy)?(dx>0?['d','s']:['a','w']):(dy>0?['a','s']:['d','w']);
      await step({keys,duration_ms:Math.max(60,Math.min(800,Math.round(Math.max(Math.abs(dx),Math.abs(dy))/7.73*1000)))});
    }
  }
  async function shoot(id,limit=100){for(let n=0;n<limit;n++){const s=await game.inspect(),e=entities(s).find(e=>e.id===id);if(!e)return;await step({keys:[],duration_ms:250,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});}throw Error('Native shots did not defeat '+id);}
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
  await step({keys:[],duration_ms:700},{name:'manor-entrance',when:()=>true});
  const shelves=map.entities.filter(e=>e.kind===0x181a),secret=shelves.find(e=>!(e.condition&(1<<23))),ordinary=shelves.find(e=>e.condition&(1<<23));
  assert(secret&&ordinary,'Generated Manor must offer both shelf types for this case');
  await walk(ordinary.pos,4);
  await step({keys:[],duration_ms:500,target:{entity:ordinary.id,camera_angle:-Math.PI/4,fire:true}});
  await refresh();game.check('ordinary bookshelf survives native staff damage',map.entities.some(e=>e.id===ordinary.id&&e.hp===ordinary.hp));
  await walk(secret.pos,3);
  await step({keys:[],duration_ms:700},{name:'secret-shelf-before',when:()=>true});
  await shoot(secret.id);
  await refresh();game.check('native staff destroys only the secret shelf and opens its blocked cell',!map.entities.some(e=>e.id===secret.id)&&!map.blocked.some(([x,y])=>x===Math.floor(secret.pos.x)&&y===Math.floor(secret.pos.y)));
  await walk(secret.pos,.3);
  await step({keys:[],duration_ms:700},{name:'secret-shelf-after',when:()=>true});
  const coffins=map.entities.filter(e=>e.kind===0x1737).sort((a,b)=>distance(a.pos,secret.pos)-distance(b.pos,secret.pos));
  game.check('secret passage leads toward a paired treasure-coffin room',coffins.length>=2&&distance(coffins[0].pos,coffins[1].pos)===4);
  const outcomes=[];
  for(const coffin of coffins){
    await refresh();await walk(coffin.pos,4);
    const before=await save('coffin-'+coffin.id+'-before'),known=new Set(entities(before).map(e=>e.id));
    await step({keys:[],duration_ms:100,target:{entity:coffin.id,camera_angle:-Math.PI/4,fire:true}});
    await step({keys:[],duration_ms:150});
    const opened=await game.inspect(),impostor=entities(opened).find(e=>e.kind===0x1722&&!known.has(e.id)&&distance(e.pos,coffin.pos)<.1);
    if(impostor){
      await shoot(impostor.id);const dead=await save('impostor-'+coffin.id);
      const brood=entities(dead).filter(e=>e.kind===0x1738&&!known.has(e.id));
      game.check('native impostor releases Lil Feratu without private loot or Manor credit',brood.length>=1&&brood.length<=4&&!entities(dead).some(e=>e.items?.some(i=>i>=0)&&distance(e.pos,coffin.pos)<1)&&dead.diagnostic.character.manors===before.diagnostic.character.manors);
      outcomes.push({id:coffin.id,impostor:true,brood:brood.length});
    }else{
      for(let n=0;n<120;n++){
        const s=await game.inspect(),e=entities(s).find(e=>e.id===coffin.id);if(!e)break;
        await step({keys:[],duration_ms:250,target:{entity:coffin.id,camera_angle:-Math.PI/4,fire:true}});
        const after=await game.inspect();
        if(e.hp>15000&&entities(after).find(x=>x.id===coffin.id)?.hp<=15000)await save('coffin-first-wave');
        if(e.hp>8000&&entities(after).find(x=>x.id===coffin.id)?.hp<=8000)await save('coffin-second-wave');
      }
      const dead=await save('genuine-'+coffin.id);assert(!entities(dead).some(e=>e.id===coffin.id));
      const bags=entities(dead).filter(e=>e.extra_stats.some(([stat,owner])=>stat===54&&owner===dead.view.session.account)&&distance(e.pos,coffin.pos)<1);
      game.check('genuine coffin defeat does not grant Manor clear credit',dead.diagnostic.character.manors===before.diagnostic.character.manors);
      outcomes.push({id:coffin.id,impostor:false,bags:bags.map(e=>e.items)});
      if(bags.length){await walk(bags[0].pos);await step({keys:[],duration_ms:700},{name:'coffin-private-reward',when:()=>true});}
    }
    if(outcomes.some(o=>o.impostor)&&outcomes.some(o=>!o.impostor&&o.bags.length))break;
  }
  await writeFile(out('coffin-outcomes.json'),JSON.stringify(outcomes));
  await refresh();const boss=map.entities.find(e=>e.kind===0x1720);assert(boss);
  await walk(boss.pos,5);const arena=await save('ruthven-arrival');
  game.check('native walk traverses populated connected rooms to Ruthven',arena.view.lifecycle.alive&&arena.diagnostic.character.shots>entry.diagnostic.character.shots);
  await step({keys:[],duration_ms:700},{name:'ruthven-before',when:()=>true});
  for(let n=0;n<500;n++){
    const s=await game.inspect(),e=entities(s).find(e=>e.id===boss.id);if(!e)break;
    if(distance(e.pos,s.view.position)>6)await walk(e.pos,4);
    else await step({keys:[],duration_ms:250,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const defeated=await save('ruthven-defeated');
  game.check('native staff defeats Ruthven and awards exactly one Manor completion',!entities(defeated).some(e=>e.id===boss.id)&&defeated.diagnostic.character.manors===entry.diagnostic.character.manors+1);
  await refresh();const exit=map.entities.find(e=>e.kind===0x71d);assert(exit,'Ruthven must publish an earned exit');
  await step({keys:[],duration_ms:700},{name:'ruthven-after',when:()=>true});
  await walk(exit.pos);
  const returned=await enter(entry.view.session.world,'boss-exit');
  game.check('earned exit returns the same living Wizard to Nexus with durable clear credit',returned.view.map.name==='Nexus'&&returned.view.lifecycle.alive&&returned.view.session.character===initial.view.session.character&&returned.diagnostic.character.manors===defeated.diagnostic.character.manors);
  await step({keys:[],duration_ms:700},{name:'manor-return',when:()=>true});
}
