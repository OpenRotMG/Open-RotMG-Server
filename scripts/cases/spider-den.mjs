import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

export const realmSeed=23;
export const fixture={class:0x30e,level:20,seed:854,world:-2,position:'spawn',equipment:[0xb08,-1,0xb05,0xac5],backpack:[0x706,-1,-1,-1,-1,-1,-1,-1],stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const output=name=>join(process.env.QA_RESULTS_DIR,name),samples=[],protections=[];
  let protectionUntil=0;
  const report=JSON.parse(await readFile(output('native/result.json'),'utf8'));
  const token=(await readFile(output('native/state/control.token'),'utf8')).trim();
  async function save(name){const s=await game.inspect();samples.push({name,state:s});await writeFile(output('journey.json'),JSON.stringify(samples));return s;}
  async function protect(){
    const response=await fetch(report.origin+'/dev/testing',{method:'POST',signal:AbortSignal.timeout(10000),headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}})});
    const result=await response.json();assert(response.ok&&result.status==='succeeded',JSON.stringify(result));
    const applied=await game.observe();protectionUntil=applied.game_ms+60000;
    protections.push({game_ms:applied.game_ms,receipt:result});await writeFile(output('protection.json'),JSON.stringify(protections));
  }
  const entities=s=>s.diagnostic.entities;
  const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
  const aim=(dx,dy)=>{const scale=Math.min(50/Math.SQRT2,270/(Math.hypot(dx,dy)*Math.SQRT2));return {x:300+(dx-dy)*scale,y:325+(dx+dy)*scale};};
  async function walk(point,fire=false,tolerance=.75){
    for(let n=0;n<150;n++){
      const s=await game.inspect(),v=s.view;let dx=point.x-v.position.x,dy=point.y-v.position.y;
      if(Math.hypot(dx,dy)<tolerance)return;
      // Centre across the next tile's axis before pressing through a one-tile opening.
      if(Math.abs(dx)>Math.abs(dy)&&Math.floor(point.x)!==Math.floor(v.position.x)&&Math.abs(dy)>.24)dx=0;
      else if(Math.abs(dy)>Math.abs(dx)&&Math.floor(point.y)!==Math.floor(v.position.y)&&Math.abs(dx)>.24)dy=0;
      if(v.time_ms>=protectionUntil)await protect();
      const keys=Math.abs(dx)>Math.abs(dy)?(dx>0?['d','s']:['a','w']):(dy>0?['a','s']:['d','w']);
      const speed=v.character.conditions&8?4:4+5.6*v.character.stats[4]/75;
      const duration_ms=Math.max(50,Math.min(fire?350:900,Math.round(Math.max(Math.abs(dx),Math.abs(dy))/speed*1000)));
      const enemy=entities(s).filter(e=>e.kind>=0x21c&&e.kind<=0x220&&distance(e.pos,v.position)<7).sort((a,b)=>distance(a.pos,v.position)-distance(b.pos,v.position))[0];
      const mouse=fire&&enemy&&duration_ms>=150?aim(enemy.pos.x-v.position.x,enemy.pos.y-v.position.y):undefined;
      // A projectile already in flight can kill a hatchling while the normal action resumes.
      await game.act({keys,duration_ms,...(mouse?{mouse}:{})});
    }
    throw Error('Native walk did not reach '+JSON.stringify(point));
  }
  async function enter(from,name){
    await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
    for(let n=0;n<8&&(await game.observe()).session.world===from;n++)await game.act({keys:[],duration_ms:500});
    const s=await save(name);assert.notEqual(s.view.session.world,from);return s;
  }
  const initial=await save('nexus');
  await game.act({keys:['1'],duration_ms:500});
  const entry=await enter(initial.view.session.world,'key-entry');
  const info=await (await fetch(report.origin+'/dev/testing',{method:'POST',signal:AbortSignal.timeout(10000),headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'world_info',world:entry.view.session.world}})})).json(),[sx,sy]=info.spawn_points[0],spawn={x:sx,y:sy};
  game.check('ordinary key consumption admits the same Wizard at Spider Den entrance',entry.view.map.name==='Spider Den'&&entry.view.session.character===initial.view.session.character&&entry.view.character.items[4]===-1&&distance(entry.view.position,spawn)<1);
  await protect();
  await game.act({keys:[],duration_ms:700},{name:'spider-entrance',when:()=>true});
  // The generated chain varies: route over the floor from the spawn to Arachna and test the sacs and eggs beside it.
  const m=await (await fetch(report.origin+'/dev/map?player='+entry.view.session.player,{headers:{Authorization:'Bearer '+token}})).json(),w=m.width,blocked=new Set(m.blocked.map(([x,y])=>y*w+x));
  const cell=p=>Math.floor(p.y)*w+Math.floor(p.x),at=c=>({x:c%w+.5,y:Math.floor(c/w)+.5}),queen=m.entities.find(e=>e.kind===0x936).pos;
  // Routes step around live sacs, so only the deliberate touch and shot hatch them.
  const sacs=new Set(m.entities.filter(e=>e.kind===0x21b).map(e=>cell(e.pos)));
  const route=(from,to,edges=new Set())=>{const prev=new Map([[from,null]]),queue=[from];for(let i=0;i<queue.length&&!prev.has(to);i++)for(const n of [queue[i]-1,queue[i]+1,queue[i]-w,queue[i]+w])if(!prev.has(n)&&!blocked.has(n)&&!sacs.has(n)&&!edges.has(n)){prev.set(n,queue[i]);queue.push(n);}assert(prev.has(to),'No floor route');const path=[];for(let c=to;c!==null;c=prev.get(c))path.unshift(c);return path;};
  const follow=async(path,fire)=>{for(let i=1;i<path.length;i++)if(i===path.length-1||path[i]-path[i-1]!==path[i+1]-path[i])await walk(at(path[i]),fire,.38);};
  const here=async()=>cell((await game.observe()).position),path=route(cell(entry.view.position),cell(queen));
  const beside=(kind,from,k)=>m.entities.filter(e=>e.kind===kind).map(e=>({e,i:path.findIndex((c,i)=>i>=from&&distance(at(c),e.pos)<k)})).filter(x=>x.i>=0).sort((a,b)=>a.i-b.i);
  const first=beside(0x21b,0,3.5).find(({e,i})=>path.slice(0,i+1).every(c=>distance(at(c),e.pos)>2.7));assert(first,'No live sac beside the route');
  await follow(path.slice(0,first.i+1),false);
  const beforeTouch=await save('before-touch'),egg=entities(beforeTouch).find(e=>e.id===first.e.id);
  assert(egg,'First natural live sac was not preserved for touch test');
  const ids=new Set(entities(beforeTouch).map(e=>e.id));
  // The recovered sac triggers at range 2, possibly alongside other nearby sacs.
  await walk(egg.pos,false,.4);
  const touched=await save('hatched-by-touch'),children=entities(touched).filter(e=>e.kind===0x21c&&e.parent===egg.id&&!ids.has(e.id));
  game.check('touching natural live sac removes it and hatches two to seven spiders without firing',!entities(touched).some(e=>e.id===egg.id)&&children.length>=2&&children.length<=7&&touched.diagnostic.character.shots===beforeTouch.diagnostic.character.shots);
  await game.act({keys:[],duration_ms:500},{name:'spider-hatchlings',when:()=>true});
  const prop=beside(0x1fe,first.i,4).find(({e})=>[-2,-1,1,2].every(d=>!blocked.has(cell(e.pos)+d)));assert(prop,'No decorative egg beside the route');
  const west={x:prop.e.pos.x-1.3,y:prop.e.pos.y};
  await follow(route(await here(),cell(west)),true);
  await walk(west,true);
  const propBefore=await save('before-decorative-egg');
  await walk({x:prop.e.pos.x+1.3,y:prop.e.pos.y},true);
  const propAfter=await save('crossed-decorative-egg');
  game.check('ordinary walk crosses the natural decorative egg without collision',propBefore.view.position.x<prop.e.pos.x-.5&&propAfter.view.position.x>prop.e.pos.x+.5);
  const live=m.entities.filter(e=>e.kind===0x21b&&entities(propAfter).some(o=>o.id===e.id));
  // Stay outside the two-tile hatch radius and choose an unobstructed axis for native fire.
  const candidates=live.flatMap(e=>[[3,0],[-3,0],[0,3],[0,-3]].flatMap(([dx,dy])=>{
    const to={x:e.pos.x+dx,y:e.pos.y+dy};
    if([1,2,3].some(n=>blocked.has(cell({x:e.pos.x+dx*n/3,y:e.pos.y+dy*n/3}))))return [];
    let approach;try{approach=route(cell(propAfter.view.position),cell(to));}catch(error){if(error.code!=='ERR_ASSERTION'||error.message!=='No floor route')throw error;return [];}
    return approach.every(c=>distance(at(c),e.pos)>2.5)?[{e,approach}]:[];
  })).sort((a,b)=>a.approach.length-b.approach.length);
  const second=candidates[0];assert(second,'No live sac with a route to a clear firing line beyond contact range');
  await follow(second.approach,false);
  const beforeShot=await save('before-shot-sac'),shotEgg=entities(beforeShot).find(e=>e.id===second.e.id);assert(shotEgg&&distance(shotEgg.pos,beforeShot.view.position)>2);
  const prior=new Set(entities(beforeShot).map(e=>e.id));
  // Six projectiles in the old hold could kill every hatchling before inspection (AC job a50a987a).
  let shot=beforeShot;
  for(let n=0;n<5&&entities(shot).some(e=>e.id===shotEgg.id);n++){
    await game.act({keys:[],duration_ms:100,target:{entity:shotEgg.id,camera_angle:-Math.PI/4,fire:true}});
    await game.act({keys:[],duration_ms:300});
    shot=await save('shot-sac-'+n);
  }
  const survivors=entities(shot).filter(e=>e.kind===0x21c&&e.parent===shotEgg.id&&!prior.has(e.id));
  game.check('ordinary staff shot hatches another natural live sac with observed offspring',!entities(shot).some(e=>e.id===shotEgg.id)&&survivors.length>=1&&survivors.length<=7);
  // Avoid hugging a concave wall: the selected client cannot enter the next tile if the player's radius overlaps the corner.
  const start=await here(),end=cell({x:queen.x,y:queen.y+8}),edges=new Set();
  for(let y=0;y<m.height;y++)for(let x=0;x<w;x++){const c=y*w+x;if(c!==start&&c!==end&&[-1,1].some(dx=>[-1,1].some(dy=>blocked.has(c+dx+dy*w)&&!blocked.has(c+dx)&&!blocked.has(c+dy*w))))edges.add(c);}
  const arenaPath=route(start,end,edges);
  // Fight in a real populated room, but do not decelerate and re-aim for every tile on the long tail.
  for(let n=0;n<8&&(await game.inspect()).diagnostic.character.kills===entry.diagnostic.character.kills;n++){
    const s=await game.inspect(),mob=entities(s).find(e=>e.kind>=0x21c&&e.kind<=0x220&&distance(e.pos,s.view.position)<6&&s.view.objects.some(o=>o.id===e.id));
    if(!mob)break;
    await game.act({keys:[],duration_ms:350,target:{entity:mob.id,camera_angle:-Math.PI/4,fire:true}});
  }
  await follow(arenaPath,false);
  await game.act({keys:[],duration_ms:700},{name:'spider-boss-approach',when:()=>true});
  const arena=await save('boss-room'),boss=entities(arena).find(e=>e.kind===0x936);assert(boss);
  game.check('entrance-to-boss route is traversable with ordinary populated rooms',arena.view.lifecycle.alive&&arena.diagnostic.character.kills>entry.diagnostic.character.kills&&arena.view.character.experience>entry.view.character.experience);
}
