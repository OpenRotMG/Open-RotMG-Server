import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x31e,level:20,seed:853,world:1,position:'spawn',equipment:[0xa84,-1,0xafc,0xacd],backpack:[0x705,-1,-1,-1,-1,-1,-1,-1],stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,root=join(out,'native'),distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
  const snapshots=[],seen=new Map(),shots=new Map();
  async function save(name){const s=await game.inspect();snapshots.push({name,...s});await writeFile(join(out,'journey.json'),JSON.stringify(snapshots));return s;}
  async function sample(input,capture){
    let pending,error;
    const record=async()=>{const s=await game.inspect();for(const e of s.view.objects)if(e.kind>=0x683&&e.kind<=0x68f)seen.set(e.id,e);for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);};
    const timer=setInterval(()=>{if(!pending)pending=record().catch(e=>{error=e;}).finally(()=>{pending=null;});},80);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;await record();
  }
  const before=await save('realm');
  await game.act({keys:['1'],duration_ms:700});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const entered=await save('entered');
  game.check('ordinary supplied key enters Pirate Cave once with the same living Knight',entered.view.map.name==='Pirate Cave'&&entered.view.session.character===before.view.session.character&&entered.view.lifecycle.alive&&entered.view.character.items[4]===-1&&entered.view.character.inventory_revision===before.view.character.inventory_revision+1);
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const token=(await readFile(join(root,'state/control.token'),'utf8')).trim();
  const response=await fetch(report.origin+'/dev/map?player='+entered.diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(response.ok);
  const map=await response.json();await writeFile(join(out,'natural-cave.json'),JSON.stringify(map));
  const blocked=new Set(map.blocked.map(p=>p.join(','))),nodes=new Set();
  for(let y=1;y<map.height;y++)for(let x=1;x<map.width;x++)if([[x,y],[x-1,y],[x,y-1],[x-1,y-1]].every(p=>!blocked.has(p.join(','))))nodes.add(y*map.width+x);
  const point=i=>({x:i%map.width,y:Math.floor(i/map.width)}),nearest=p=>[...nodes].reduce((a,b)=>distance(point(a),p)<distance(point(b),p)?a:b);
  function route(start,goal){
    const from=nearest(start),to=nearest(goal),parents=new Map([[from,null]]),queue=[from];
    for(let n=0;n<queue.length&&!parents.has(to);n++)for(const next of [queue[n]+1,queue[n]-1,queue[n]+map.width,queue[n]-map.width])if(nodes.has(next)&&!parents.has(next)){parents.set(next,queue[n]);queue.push(next);}
    assert(parents.has(to),'Natural cave destination is unreachable');
    const path=[];for(let i=to;i!==null;i=parents.get(i))path.push(i);path.reverse();
    return path.filter((i,n)=>n===0||n===path.length-1||i-path[n-1]!==path[n+1]-i).map(point);
  }
  async function walk(goal,combat=true){
    for(const p of route((await game.observe()).position,goal)){
      let reached=false;
      for(let n=0;n<40;n++){
        const v=(await game.inspect()).view;assert(v.lifecycle.alive,'Knight died during cave traversal');
        const dx=p.x-v.position.x,dy=p.y-v.position.y;if(Math.max(Math.abs(dx),Math.abs(dy))<.6){reached=true;break;}
        const horizontal=Math.abs(dx)>Math.abs(dy),delta=horizontal?dx:dy;
        const input={keys:horizontal?(delta>0?['d','s']:['a','w']):(delta>0?['a','s']:['d','w']),duration_ms:Math.max(80,Math.min(500,Math.round(Math.abs(delta)/7.5*1000)))};
        const enemy=v.objects.filter(e=>e.hp>0&&e.kind>=0x683&&e.kind<=0x689&&distance(v.position,e.position)<3).sort((a,b)=>distance(v.position,a.position)-distance(v.position,b.position))[0];
        if(combat&&enemy)input.target={entity:enemy.id,camera_angle:-Math.PI/4,fire:true};
        await sample(input);
      }
      assert(reached,'Native movement did not reach '+JSON.stringify(p));
    }
  }
  await sample({keys:[],duration_ms:900},{name:'pirate-entry',when:()=>true});
  const boss=map.entities.find(e=>e.kind===0x927);assert(boss,'Natural Dreadstump missing');
  const remaining=map.entities.filter(e=>e.kind>=0x68a&&e.kind<=0x68f);
  assert(remaining.length,'Natural cave has no passive population');
  while(remaining.length){
    const v=await game.observe();remaining.sort((a,b)=>distance(v.position,a.pos)-distance(v.position,b.pos));const e=remaining.shift();
    await walk(e.pos);await sample({keys:[],duration_ms:1100});
  }
  game.check('ordinary room traversal encounters naturally generated officers fighters and passive fauna',[ [0x683,0x686],[0x687,0x689],[0x68a,0x68f] ].every(([lo,hi])=>[...seen.values()].some(e=>e.kind>=lo&&e.kind<=hi)));
  await sample({keys:[],duration_ms:900},{name:'pirate-roster',when:()=>true});
  await walk({x:boss.pos.x-3,y:boss.pos.y},false);
  await sample({keys:[],duration_ms:1500},{name:'pirate-boss-intact',when:()=>true});
  const intact=await save('boss-intact'),live=intact.diagnostic.entities.find(e=>e.id===boss.id);
  game.check('untouched selected 800-HP Dreadstump emits no attack',live?.hp===800&&![...shots.values()].some(p=>p.owner===boss.id));
  // The selected client fixes projectile values; the HP gate is a comparator: https://github.com/trapped/rotmg_svr/blob/593e490803fe0ad168ead408cacd5287d778acfe/wServer/logic/db/BehaviorDb.PirateCave.cs#L17-L32
  let damaged;
  await walk(live.pos,false);
  for(let n=0;n<8;n++){
    await sample({keys:[],duration_ms:100,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});await sample({keys:[],duration_ms:250});
    damaged=(await game.inspect()).diagnostic.entities.find(e=>e.id===boss.id);assert(damaged,'Boss died before its attack phase');if(damaged.hp<640)break;
  }
  game.check('ordinary sword damage crosses Dreadstump attack gate',damaged.hp<640);
  await sample({keys:[],duration_ms:2500},{name:'pirate-boss-attacking',when:()=>true});
  const bossShots=[...shots.values()].filter(p=>p.owner===boss.id);
  game.check('damaged Dreadstump sends both selected projectile definitions',bossShots.some(p=>p.speed===75&&p.lifetime_ms===1500)&&bossShots.some(p=>p.speed===130&&p.lifetime_ms===1100));
  for(let n=0;n<20;n++){
    const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===boss.id);if(!e)break;
    if(distance(s.view.position,e.pos)>3)await walk(e.pos,false);
    await sample({keys:[],duration_ms:500,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const defeated=await save('boss-defeated');
  game.check('native combat defeats Dreadstump and grants exactly one cave clear',!defeated.diagnostic.entities.some(e=>e.id===boss.id)&&defeated.view.character.pirate_caves===entered.view.character.pirate_caves+1&&defeated.view.character.experience>entered.view.character.experience);
  const portal=defeated.diagnostic.entities.find(e=>e.kind===0x704);assert(portal,'Completion Realm Portal missing');
  const bags=defeated.diagnostic.entities.filter(e=>e.items?.some(i=>i!==-1));
  await writeFile(join(out,'encounters.json'),JSON.stringify({seen:[...seen.values()],shots:[...shots.values()],bags,boss}));
  await sample({keys:[],duration_ms:1500});
  game.check('passive fauna emit no projectiles and cave credit does not repeat',![...shots.values()].some(p=>seen.get(p.owner)?.kind>=0x68a)&&(await game.observe()).character.pirate_caves===defeated.view.character.pirate_caves);
  const bag=bags.sort((a,b)=>distance(defeated.view.position,a.pos)-distance(defeated.view.position,b.pos))[0];assert(bag,'No naturally rolled cave reward available');
  await walk(bag.pos,false);await sample({keys:[],duration_ms:900},{name:'pirate-loot',when:()=>true});
  const slot=bag.items.findIndex(i=>i!==-1),item=bag.items[slot],inventory=await game.observe();
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},to:{x:634,y:428}}});
  const picked=await save('reward-picked');
  game.check('native inventory drag acquires an eligible naturally rolled cave reward once',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===inventory.character.inventory_revision+1&&picked.diagnostic.entities.find(e=>e.id===bag.id)?.items[slot]!==item);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},to:{x:678,y:428}}});
  game.check('empty reward slot cannot duplicate loot',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  await walk(portal.pos,false);await sample({keys:[],duration_ms:900},{name:'pirate-realm-exit',when:()=>true});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const returned=await save('returned');
  game.check('completion portal returns the same living character to the opening Realm',returned.view.map.name==='Realm'&&returned.view.session.world===before.view.session.world&&returned.view.session.character===before.view.session.character&&returned.view.lifecycle.alive&&returned.view.character.pirate_caves===defeated.view.character.pirate_caves&&returned.view.character.items[4]===item);
  await sample({keys:[],duration_ms:900},{name:'pirate-returned',when:()=>true});
}
