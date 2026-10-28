import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';

process.env.RUST_LOG='info,openrotmg::world=debug';
export const fixture={class:0x31e,level:20,seed:858,world:1,position:'spawn',equipment:[0xa84,0xb22,0xafc,0xacd],backpack:[0x701,...Array(7).fill(-1)],stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,root=join(out,'native'),distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
  const snapshots=[],shots=new Map(),seen=new Map(),escorts=new Map(),splits=[],traps=new Set();
  async function save(name){const s=await game.inspect();snapshots.push({name,...s});await writeFile(join(out,'journey.json'),JSON.stringify(snapshots));return s;}
  let prior,protection=0;
  async function record(){
    const s=await game.inspect();
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
  async function sample(input,capture){
    let pending,error;const before=await record();
    if(!(before.view.character.conditions&(1<<23))){
      const path=join(out,'protection-'+(++protection)+'.json');
      await writeFile(path,JSON.stringify({id:'lair-protection-'+protection,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}}));
      execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path]);
    }
    const timer=setInterval(()=>{if(!pending)pending=record().catch(e=>{error=e;}).finally(()=>{pending=null;});},120);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;await record();
  }
  const before=await save('realm');
  await game.act({keys:['1'],duration_ms:700});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const entered=await save('entered');
  game.check('supplied key enters Undead Lair once with the same living Knight',entered.view.map.name==='Undead Lair'&&entered.view.session.character===before.view.session.character&&entered.view.lifecycle.alive&&entered.view.character.items[4]===-1&&entered.view.character.inventory_revision===before.view.character.inventory_revision+1);
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
        await sample({keys,duration_ms:Math.max(50,Math.min(450,Math.round(Math.abs(delta)/7.5*1000))),...(combat&&enemy?{target:{entity:enemy.id,camera_angle:-Math.PI/4,fire:true}}:{})});
      }
      assert(reached,'Native movement did not reach '+JSON.stringify(p));
    }
  }
  await sample({keys:[],duration_ms:900},{name:'lair-entry',when:()=>true});
  async function emissions(id){
    return (await readFile(join(root,'server.log'),'utf8')).split('\n').slice(0,-1).filter(line=>line.startsWith('{')).map(line=>JSON.parse(line)).filter(e=>e.fields?.message==='Lair trap detonated'&&e.fields.entity===id);
  }
  for(const kind of [0xda2,0xda3]){
    const v=await game.observe(),trap=map.entities.filter(e=>e.kind===kind).sort((a,b)=>distance(v.position,a.pos)-distance(v.position,b.pos))[0];assert(trap,'Natural lair trap missing');
    await walk(trap.pos,false);await sample({keys:[],duration_ms:1100},{name:'lair-trap-'+kind,when:()=>true});
    const fired=await emissions(trap.id);
    game.check(kind+': ordinary movement triggers one logged volley and removal',fired.length===1&&fired[0].fields.shots>0&&!(await game.inspect()).diagnostic.entities.some(e=>e.id===trap.id));
    await sample({keys:[],duration_ms:1100});
    game.check(kind+': removed trap never repeats its volley',(await emissions(trap.id)).length===1);
  }
  const boss=map.entities.find(e=>e.kind===0xd90);assert(boss,'Natural Septavius missing');
  await walk({x:boss.pos.x-6,y:boss.pos.y});
  await sample({keys:[],duration_ms:900},{name:'lair-boss-arrival',when:()=>true});
  const arrival=await save('boss-arrival'),started=arrival.diagnostic.entities.find(e=>e.id===boss.id);assert(started?.activated!==null);
  const observer=await game.join({...fixture,world:entered.view.session.world,position:entered.view.position,equipment:[-1,-1,0xafc,0xacd],backpack:Array(8).fill(-1)});
  const joined=await save('late-join');
  game.check('late native join preserves boss identity activation and HP',joined.diagnostic.entities.find(e=>e.id===boss.id)?.activated===started.activated&&joined.diagnostic.entities.find(e=>e.id===boss.id)?.hp===started.hp&&(await observer.observe()).session.world===entered.view.session.world);
  const phases=new Set();
  for(let n=0;n<160;n++){
    const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===boss.id);assert(e&&s.view.lifecycle.alive,'Knight or Septavius lost during phase observation');
    const elapsed=s.diagnostic.time_ms-e.activated;phases.add(e.stage%4);if(elapsed>=52000)break;
    const angle=Math.atan2(s.view.position.y-e.pos.y,s.view.position.x-e.pos.x)+.12,target={x:e.pos.x+7*Math.cos(angle),y:e.pos.y+7*Math.sin(angle)};
    const dx=target.x-s.view.position.x,dy=target.y-s.view.position.y,horizontal=Math.abs(dx)>Math.abs(dy),delta=horizontal?dx:dy;
    const keys=horizontal?(delta>0?['d','s']:['a','w']):(delta>0?['a','s']:['d','w']);
    const minion=s.diagnostic.entities.find(c=>c.kind>=0xdaa&&c.kind<=0xdb2&&distance(s.view.position,c.pos)<3);
    await sample({keys,duration_ms:350,...(minion?{target:{entity:minion.id,camera_angle:-Math.PI/4,fire:true}}:{})},n===5?{name:'lair-phases',when:()=>true}:undefined);
  }
  await writeFile(join(out,'encounters.json'),JSON.stringify({seen:[...seen.values()],shots:[...shots.values()],escorts:[...escorts.values()],splits,traps:[...traps],phases:[...phases]}));
  game.check('all four Septavius phase windows execute without a late-join restart',phases.size===4);
  await save('phases-observed');
  const bossShots=[...shots.values()].filter(p=>p.owner===boss.id);
  game.check('all four configured Septavius projectile types are observed',[[80,1500],[40,4000],[90,2600],[57,3000]].every(([speed,life])=>bossShots.some(p=>p.speed===speed&&p.lifetime_ms===life)));
  game.check('both escort waves retain the same boss owner',[[0xdaa,0xdaf],[0xdb0,0xdb2]].every(([lo,hi])=>[...escorts.values()].some(e=>e.kind>=lo&&e.kind<=hi&&e.parent===boss.id)));
  for(let n=0;n<150;n++){
    const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===boss.id);if(!e)break;
    assert(s.view.lifecycle.alive,'Knight died fighting Septavius');
    if(distance(s.view.position,e.pos)>3)await walk(e.pos,false);
    await sample({keys:[' '],duration_ms:350,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const defeated=await save('boss-defeated');
  game.check('native sword defeats Septavius and grants exactly one lair clear',!defeated.diagnostic.entities.some(e=>e.id===boss.id)&&defeated.diagnostic.character.dungeons[2]===entered.diagnostic.character.dungeons[2]+1&&defeated.view.character.experience>entered.view.character.experience);
  const portal=defeated.diagnostic.entities.find(e=>e.kind===0x704);assert(portal,'Completion Realm Portal missing');
  const bag=defeated.diagnostic.entities.find(e=>e.items?.includes(0xa35));assert(bag,'Eligible Wisdom reward missing');
  await walk(bag.pos);await sample({keys:[],duration_ms:900},{name:'lair-reward',when:()=>true});
  const slot=bag.items.indexOf(0xa35),inventory=await game.observe();
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},to:{x:634,y:428}}});
  const picked=await save('wisdom-picked');
  game.check('native drag acquires eligible Wisdom once',picked.view.character.items[4]===0xa35&&picked.view.character.inventory_revision===inventory.character.inventory_revision+1&&picked.diagnostic.entities.find(e=>e.id===bag.id)?.items[slot]!==0xa35);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},to:{x:678,y:428}}});
  game.check('empty reward slot cannot duplicate Wisdom',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  await walk(portal.pos);await sample({keys:[],duration_ms:900},{name:'lair-exit',when:()=>true});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const returned=await save('returned');
  game.check('Realm exit retains the same living character reward and single clear',returned.view.map.name==='Realm'&&returned.view.session.world===before.view.session.world&&returned.view.session.character===before.view.session.character&&returned.view.lifecycle.alive&&returned.diagnostic.character.dungeons[2]===defeated.diagnostic.character.dungeons[2]&&returned.view.character.items[4]===0xa35);
  await sample({keys:[],duration_ms:900},{name:'lair-returned',when:()=>true});
}
