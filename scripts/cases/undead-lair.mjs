import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';

process.env.RUST_LOG='info,openrotmg::world=debug';
export const fixture={class:0x31e,level:20,seed:858,world:1,position:'spawn',equipment:[0xa84,0xb22,0xafc,0xacd],backpack:[0x701,...Array(7).fill(-1)],stats:[770,252,50,40,50,50,75,50]};
export async function runLair(game,segment){
  const out=process.env.QA_RESULTS_DIR,root=join(out,'native'),distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
  const snapshots=[],shots=new Map(),seen=new Map(),escorts=new Map(),splits=[],traps=new Set();
  async function save(name){const s=await game.inspect();snapshots.push({name,...s});await writeFile(join(out,'journey.json'),JSON.stringify(snapshots));return s;}
  let prior,protection=0;
  const timeline=segment!=='clear';
  async function record(){
    const s=await game.inspect();
    if(!timeline)return s;
    for(const e of s.view.objects)if(e.kind>=0xd91&&e.kind<=0xdb2)seen.set(e.id,e);
    for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);
    for(const e of s.diagnostic.entities)if(e.kind>=0xdaa&&e.kind<=0xdb2&&e.parent!==null)escorts.set(e.id,e);
    if(prior)for(const e of prior.diagnostic.entities){
      if(![0xd9b,0xd9c,0xd9e,0xda2,0xda3].includes(e.kind)||s.diagnostic.entities.some(c=>c.id===e.id))continue;
      if(e.kind===0xda2||e.kind===0xda3)traps.add(e.id);
      else {const children=s.diagnostic.entities.filter(c=>c.kind===(e.kind===0xd9b?0xd9c:e.kind===0xd9e?0xd9f:0xd9d)&&distance(c.pos,e.pos)<2);if(children.length)splits.push({parent:e.id,kind:e.kind,children:children.map(c=>c.id)});}
    }
    prior=s;return s;
  }
  let shielded=0;
  async function sample(input,capture){
    let pending,error,before=await record();
    // Protection cannot be renewed while active, and loaded acts can overrun: idle out the window, then renew.
    while(before.view.time_ms<shielded&&shielded-before.view.time_ms<input.duration_ms+5000){await game.act({keys:[],duration_ms:Math.min(10000,Math.max(50,shielded-before.view.time_ms))});before=await record();}
    if(!(before.view.character.conditions&(1<<23))){shielded=before.view.time_ms+60000;
      const path=join(out,'protection-'+(++protection)+'.json');
      await writeFile(path,JSON.stringify({id:'lair-protection-'+protection,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}}));
      execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path]);
    }
    const timer=setInterval(()=>{if(!pending)pending=record().catch(e=>{error=e;}).finally(()=>{pending=null;});},120);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;await record();
  }
  const before=await save(segment==='traps'?'realm':'prepared-antechamber');
  if(segment==='traps'){
    await game.act({keys:['1'],duration_ms:700});
    await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  }
  const entered=await save('entered');
  if(segment==='traps')game.check('supplied key enters Undead Lair once with the same living Knight',entered.view.map.name==='Undead Lair'&&entered.view.session.character===before.view.session.character&&entered.view.lifecycle.alive&&entered.view.character.items[4]===-1&&entered.view.character.inventory_revision===before.view.character.inventory_revision+1);
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const response=await fetch(config.origin+'/dev/map?player='+entered.diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(response.ok);
  const map=await response.json();await writeFile(join(out,'natural-lair.json'),JSON.stringify(map));
  const blocked=new Set(map.blocked.map(p=>p.join(','))),nodes=new Set();
  for(let y=1;y<map.height;y++)for(let x=1;x<map.width;x++)if([[x,y],[x-1,y],[x,y-1],[x-1,y-1]].every(p=>!blocked.has(p.join(','))))nodes.add(y*map.width+x);
  const point=i=>({x:i%map.width,y:Math.floor(i/map.width)}),nearest=p=>[...nodes].reduce((a,b)=>distance(point(a),p)<distance(point(b),p)?a:b);
  function route(start,goal){
    const from=nearest(start),to=nearest(goal),parents=new Map([[from,null]]),queue=[from];
    for(let n=0;n<queue.length&&!parents.has(to);n++)for(const next of [queue[n]+1,queue[n]-1,queue[n]+map.width,queue[n]-map.width])if(nodes.has(next)&&!parents.has(next)){parents.set(next,queue[n]);queue.push(next);}
    assert(parents.has(to),'Natural lair destination is unreachable');
    const path=[];for(let i=to;i!==null;i=parents.get(i))path.push(i);path.reverse();
    return path.filter((i,n)=>n===0||n===path.length-1||i-path[n-1]!==path[n+1]-i).map(point);
  }
  async function walk(goal,combat=true){
    for(const p of route((await game.observe()).position,goal)){
      let reached=false;
      for(let n=0;n<80;n++){
        const s=await game.inspect(),v=s.view;assert(v.lifecycle.alive,'Knight died during lair traversal');
        const dx=p.x-v.position.x,dy=p.y-v.position.y;if(Math.max(Math.abs(dx),Math.abs(dy))<.3){reached=true;break;}
        const horizontal=Math.abs(dx)>=.3&&(Math.abs(dy)<.3||Math.abs(dx)<Math.abs(dy)),delta=horizontal?dx:dy;
        const keys=horizontal?(delta>0?['d','s']:['a','w']):(delta>0?['a','s']:['d','w']);
        const enemy=s.diagnostic.entities.filter(e=>e.hp>0&&e.kind>=0xd91&&e.kind<=0xdb2&&e.kind!==0xda2&&e.kind!==0xda3&&distance(v.position,e.pos)<3.2).sort((a,b)=>distance(v.position,a.pos)-distance(v.position,b.pos))[0];
        // Keys follow the mouse press inside one hold, so a short combined act can expire before its keys are dispatched; fire separately.
        if(combat&&enemy)await sample({keys:[],duration_ms:250,target:{entity:enemy.id,camera_angle:-Math.PI/4,fire:true}});
        await sample({keys,duration_ms:Math.max(50,Math.min(450,Math.round(Math.abs(delta)/7.5*1000)))});
      }
      assert(reached,'Native movement did not reach '+JSON.stringify(p));
    }
  }
  if(segment==='traps'){
    await sample({keys:[],duration_ms:900},{name:'lair-entry',when:()=>true});
    // XML Lair traps Explode once and remove themselves (actors.xml:10031-10038); the vanished trap's volley is only in the audit log.
    const emissions=async id=>(await readFile(join(root,'server.log'),'utf8')).split('\n').filter(line=>line.includes('volley_attempt')&&line.includes(String(id))).flatMap(line=>JSON.parse(JSON.parse(line).fields.record).data.facts).filter(f=>f.event==='volley_attempt'&&f.values.owner===id);
    for(const kind of [0xda2,0xda3]){
      const v=await game.observe(),trap=map.entities.filter(e=>e.kind===kind).sort((a,b)=>distance(v.position,a.pos)-distance(v.position,b.pos))[0];assert(trap,'Natural lair trap missing');
      await walk(trap.pos,false);await sample({keys:[],duration_ms:1100},{name:'lair-trap-'+kind,when:()=>true});
      const fired=(await emissions(trap.id)).length;
      game.check(kind+': ordinary movement triggers one volley and removal',fired===1&&!(await game.inspect()).diagnostic.entities.some(e=>e.id===trap.id));
      await sample({keys:[],duration_ms:1100});
      game.check(kind+': removed trap never repeats its volley',(await emissions(trap.id)).length===fired);
    }
    return;
  }
  const boss=map.entities.find(e=>e.kind===0xd90);assert(boss,'Prepared Septavius missing');
  game.check('fresh seeded approach is six tiles west of the living boss',boss.hp>0&&Math.abs(boss.pos.x-entered.view.position.x-6)<.1&&Math.abs(boss.pos.y-entered.view.position.y)<.1);
  await sample({keys:[],duration_ms:900},{name:'lair-boss-arrival',when:()=>true});
  if(segment==='phases'){
    const arrival=await save('boss-arrival'),started=arrival.diagnostic.entities.find(e=>e.id===boss.id);assert(started?.activated!==null);
    // Preserve the original late join from the entrance, outside the prepared fighter's boss volleys.
    const observer=await game.join({...fixture,world:entered.view.session.world,position:'spawn',equipment:[-1,-1,0xafc,0xacd],backpack:Array(8).fill(-1)});
    const joined=await save('late-join');
    game.check('late native join preserves boss identity activation and HP',joined.diagnostic.entities.find(e=>e.id===boss.id)?.activated===started.activated&&joined.diagnostic.entities.find(e=>e.id===boss.id)?.hp===started.hp&&(await observer.observe()).session.world===entered.view.session.world);
    const phases=new Set();
    for(let n=0;n<20;n++){
      const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===boss.id);assert(e&&s.view.lifecycle.alive,'Knight or Septavius lost during phase observation');
      const elapsed=s.diagnostic.time_ms-e.activated;phases.add(s.diagnostic.xml[boss.id]?.states[1]?.name);if(elapsed>=62000||phases.has('Summon_the_undead'))break;
      // Stay within the targeted Shoot range without spending a movement receipt on every observation.
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,d=Math.hypot(dx,dy);
      if(d>6){
        const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
        const keys=[Math.abs(x)>.4142*Math.abs(y)?(x>0?'d':'a'):null,Math.abs(y)>.4142*Math.abs(x)?(y>0?'s':'w'):null].filter(Boolean);
        await sample({keys,duration_ms:Math.min(650,Math.round((d-4.5)*130))});
      }
      await sample({keys:[],duration_ms:3100},n===5?{name:'lair-phases',when:()=>true}:undefined);
    }
    await writeFile(join(out,'encounters.json'),JSON.stringify({seen:[...seen.values()],shots:[...shots.values()],escorts:[...escorts.values()],splits,traps:[...traps],phases:[...phases]}));
    game.check('all four Septavius phase windows execute without a late-join restart',['Daisy_attack','Clear_the_room','Pestilence','Summon_the_undead'].every(n=>phases.has(n)));
    await save('phases-observed');
    const bossShots=[...shots.values()].filter(p=>p.owner===boss.id);
    game.check('all four configured Septavius projectile types are observed',[[80,1500],[40,4000],[90,2600],[57,3000]].every(([speed,life])=>bossShots.some(p=>p.speed===speed&&p.lifetime_ms===life)));
    // XML Septavius summons one owned Ghost wave (actors.xml:9852-9854); the Lair Ghosts are natural dungeon population.
    game.check('summoned escort wave retains the same boss owner',[...escorts.values()].some(e=>e.kind>=0xdb0&&e.kind<=0xdb2&&e.parent===boss.id));
    const witness=await observer.observe();
    game.check('late observer remains alive in the same lair through the phase windows',witness.lifecycle.alive&&witness.session.world===entered.view.session.world);
    return;
  }
  assert.equal(segment,'clear');
  for(let n=0;n<150;n++){
    const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===boss.id);if(!e)break;
    assert(s.view.lifecycle.alive,'Knight died fighting Septavius');
    if(distance(s.view.position,e.pos)>3)await walk(e.pos,false);
    await sample({keys:[' '],duration_ms:350,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const defeated=await save('boss-defeated');
  game.check('native sword defeats Septavius and grants exactly one lair clear',!defeated.diagnostic.entities.some(e=>e.id===boss.id)&&defeated.diagnostic.character.dungeons[2]===entered.diagnostic.character.dungeons[2]+1&&defeated.view.character.experience>entered.view.character.experience);
  const portals=defeated.diagnostic.entities.filter(e=>e.kind===0x704||e.kind===0x71d);
  assert(portals.length===1&&portals[0].kind===0x704,'Expected exactly one Realm completion portal');
  const [portal]=portals;
  // Recovered Wisdom has min=1, so verify that specific earned potion (actors.xml:9733; data/objects.xml).
  const bag=defeated.diagnostic.entities.find(e=>e.items?.includes(0xa35)&&e.extra_stats.some(([stat,owner])=>stat===54&&owner===defeated.view.session.account));assert(bag,'Private Septavius reward missing');
  await walk(bag.pos,false);
  // The native ground inventory selects the nearest container, which may be a public escort drop.
  const atReward=s=>{
    const {position}=s.view,d=distance(position,bag.pos);
    return d<.55&&s.diagnostic.entities.every(e=>e.id===bag.id||!e.items?.some(item=>item>=0)||distance(position,e.pos)>d+.05);
  };
  for(let n=0;n<10;n++){
    const s=await game.inspect();if(atReward(s))break;
    const {position}=s.view,dx=bag.pos.x-position.x,dy=bag.pos.y-position.y;
    const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
    const keys=[Math.abs(x)>.4142*Math.abs(y)?(x>0?'d':'a'):null,Math.abs(y)>.4142*Math.abs(x)?(y>0?'s':'w'):null].filter(Boolean);
    assert(keys.length,'Private reward overlaps another container');
    await sample({keys,duration_ms:Math.max(50,Math.min(200,Math.round(Math.hypot(dx,dy)/7.5*850)))});
  }
  game.check('earned private Wisdom bag is the nearest native container',atReward(await save('wisdom-approach')));
  await sample({keys:[],duration_ms:900},{name:'lair-reward',when:()=>true});
  const slot=bag.items.indexOf(0xa35),reward=bag.items[slot],inventory=await game.observe();
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},to:{x:634,y:428}}});
  const picked=await save('wisdom-picked');
  game.check('native drag acquires the private reward once',picked.view.character.items[4]===reward&&picked.view.character.inventory_revision===inventory.character.inventory_revision+1&&picked.diagnostic.entities.find(e=>e.id===bag.id)?.items[slot]!==reward);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},to:{x:678,y:428}}});
  game.check('empty reward slot cannot duplicate the reward',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  await walk(portal.pos,false);await sample({keys:[],duration_ms:900},{name:'lair-exit',when:()=>true});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const returned=await save('returned');
  game.check('Realm exit retains the same living character reward and single clear',returned.view.map.name==='Realm'&&returned.view.session.world!==entered.view.session.world&&returned.view.session.character===before.view.session.character&&returned.view.lifecycle.alive&&returned.diagnostic.character.dungeons[2]===defeated.diagnostic.character.dungeons[2]&&returned.view.character.items[4]===reward);
  await sample({keys:[],duration_ms:900},{name:'lair-returned',when:()=>true});
}

export const run=game=>runLair(game,'traps');

// Seed 12351's generated map has Septavius at (15.5,120.5). The case checks the
// actual boss in that map before acting; WorldInfo does not export his marker.
export const preparedLair={...fixture,world:'undead-lair',worldSeed:12351,backpack:Array(8).fill(-1),position:world=>{
  assert(world.width===205&&world.height===153,'Unexpected seeded Lair geometry');
  return {x:9.5,y:120.5};
}};
