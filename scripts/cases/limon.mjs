import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {route as nativeRoute} from './native-route.mjs';

export const fixture={class:0x31e,level:20,seed:857,world:'sprite-world',worldSeed:1,position:world=>{const b=world.encounter_markers.find(e=>e.kind===0xd06);assert(b);return{x:b.position.x+1,y:b.position.y};},equipment:[0xb0b,-1,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const samples=[],shots=new Map(),corners=new Map();let protectedUntil=0;
  const dist=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y),entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  // Isolate the boss/reward checks from Ice Spinner's repeated Slow on flowing tiles.
  async function protect(){const now=(await game.observe()).game_ms;if(now<protectedUntil)return;const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}}),signal:AbortSignal.timeout(10000)});const receipt=await r.json();assert(r.ok&&receipt.status==='succeeded',JSON.stringify(receipt));protectedUntil=now+60000;}
  async function record(){const s=await game.inspect(),{tiles,...view}=s.view;samples.push({...s,view});for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);for(const e of s.diagnostic.entities)if(e.kind>=0xd07&&e.kind<=0xd0a)corners.set(e.id,e);return s;}
  async function act(input,capture){await protect();let pending,error;const timer=setInterval(()=>{if(!pending)pending=record().catch(e=>{error=e;}).finally(()=>pending=null);},200);try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}if(error)throw error;const s=await record();await writeFile(join(out,'limon.json'),JSON.stringify({samples,shots:[...shots.values()],corners:[...corners.values()]}));return s;}
  async function walk(goal,range=.55){for(let n=0;n<50;n++){const v=await game.observe(),p=typeof goal==='number'?entity(await game.inspect(),goal)?.pos:goal;if(!p)return false;const dx=p.x-v.position.x,dy=p.y-v.position.y;if(dist(v.position,p)<range)return true;const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');await act({keys,duration_ms:Math.max(50,Math.min(300,Math.round(dist(v.position,p)/7.5*600)))});}throw Error('Native movement did not reach '+JSON.stringify(goal));}
  const initial=await record(),boss=initial.diagnostic.entities.find(e=>e.kind===0xd06);assert(boss);
  game.check('natural Limon starts with selected 9000 HP',boss.hp===9000);
  await act({keys:[],duration_ms:1000},{name:'limon-arrival',when:v=>v.lifecycle.alive});
  // Recovered named stages and shield children, not legacy numeric stage slots (spriteWorld.xml:177ff; actors.xml:5529-5609).
  const phase=(s,name)=>s.diagnostic.xml[boss.id]?.states.some(state=>state.name===name);
  for(let n=0;n<40&&!samples.some(s=>phase(s,'Summon_the_sprites'));n++)await act({keys:[],duration_ms:1000});
  await act({keys:[],duration_ms:1500});
  game.check('native observer sees circle, box and summon phases',['circle_player','set_up_the_box','Summon_the_sprites'].every(name=>samples.some(s=>phase(s,name))));
  game.check('orbit moves Limon and publishes her selected Red Fire',samples.some(s=>phase(s,'circle_player')&&dist(entity(s,boss.id).pos,boss.home)>.2)&&[...shots.values()].some(p=>p.owner===boss.id&&p.speed===80&&p.lifetime_ms===2300));
  const boxElements=s=>s.diagnostic.entities.filter(e=>e.parent===boss.id&&e.kind>=0xd07&&e.kind<=0xd0a);
  // ObjectToss publishes new actors before their first update applies the XML root condition.
  const box=samples.find(s=>{const children=boxElements(s);return children.length===8&&children.every(e=>e.condition&(1<<24));});
  assert(box,'Eight natural box elements never became invulnerable');
  const elements=boxElements(box);
  game.check('box creates two typed invulnerable radial squares',elements.every(e=>e.hp===100000&&(e.condition&(1<<24)))&&[0xd07,0xd08,0xd09,0xd0a].every(k=>elements.filter(e=>e.kind===k).length===2)&&elements.filter(e=>Math.abs(dist(e.home,entity(box,boss.id).pos)-9.5)<.01).length===4&&elements.filter(e=>Math.abs(dist(e.home,entity(box,boss.id).pos)-14)<.01).length===4);
  game.check('box alternates shielded and vulnerable Limon',samples.some(s=>phase(s,'set_up_the_box')&&(entity(s,boss.id).condition&(1<<24)))&&samples.some(s=>phase(s,'set_up_the_box')&&!(entity(s,boss.id).condition&(1<<24))));
  game.check('elements fire selected White Flame',elements.some(e=>[...shots.values()].some(p=>p.owner===e.id&&p.speed===90&&p.lifetime_ms===1940)));
  game.check('elements expire and ordinary Ice and Magic Sprites appear',samples.some(s=>phase(s,'Summon_the_sprites')&&elements.every(e=>!entity(s,e.id)))&&[0x625,0x626].every(k=>samples.some(s=>s.diagnostic.entities.some(e=>e.parent===boss.id&&e.kind===k))));
  assert(await walk(boss.id,2),'Limon missing before combat');await act({keys:[],duration_ms:900},{name:'limon-summons',when:()=>true});
  for(let n=0;n<100;n++){
    const s=await game.inspect(),b=entity(s,boss.id);if(!b)break;
    // An earlier swing can land while walking; confirm the actual defeat and clear below.
    if(dist(s.view.position,b.pos)>2.5&&!await walk(boss.id,2))break;
    await act({keys:[],duration_ms:500,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const defeated=await record();
  game.check('native sword damage defeats Limon and grants one Sprite clear',!entity(defeated,boss.id)&&defeated.diagnostic.character.dungeons[1]===initial.diagnostic.character.dungeons[1]+1&&defeated.view.character.experience>initial.view.character.experience);
  const portal=defeated.diagnostic.entities.find(e=>e.kind===0x70e);assert(portal,'Glowing Realm Portal missing');
  const bag=defeated.diagnostic.entities.find(e=>e.items?.includes(0xa4c)&&e.extra_stats.some(([stat,owner])=>stat===54&&owner===defeated.view.session.account));assert(bag,'Private Dexterity reward missing');
  const response=await fetch(config.origin+'/dev/map?player='+defeated.view.session.player,{headers:{Authorization:'Bearer '+token}});assert(response.ok,'Cannot inspect the loot approach');
  const map=await response.json();
  const blocked=new Set(map.blocked.map(p=>p.join(',')));
  const ground=new Map(defeated.view.tiles.map(t=>[`${t.x},${t.y}`,t.ground]));
  const tile=p=>`${Math.floor(p.x)},${Math.floor(p.y)}`;
  // Account for Alpha Square flow (data/grounds.xml:1028-1074) while the drag spans multiple ticks.
  const push=new Map([[0xa0,[0,0]],[0xa1,[-2.5,0]],[0xa2,[2.5,0]],[0xa3,[0,-2.5]],[0xa4,[0,2.5]]]);
  function margin(p,target,other){
    let min=Infinity;
    // Resume and release can advance several more ticks than the scheduled pointer hold (AC job 4f188dd1).
    for(let n=0;n<=12;n++){
      const vector=push.get(ground.get(tile(p)));if(!vector||blocked.has(tile(p)))return -Infinity;
      // The SWF can hide the panel even within the server's swap limit (AC de59cd86: stationary at 1.10 tiles).
      min=Math.min(min,.95-dist(p,target),dist(p,other)-dist(p,target));
      p={x:p.x+vector[0]*.05,y:p.y+vector[1]*.05};
    }
    return min;
  }
  function nearSide(target,other){
    const goals=[];
    for(let y=Math.floor(target.y*2)-3;y<=target.y*2+3;y++)for(let x=Math.floor(target.x*2)-3;x<=target.x*2+3;x++){
      const p={x:x/2,y:y/2},clearance=margin(p,target,other);
      if(clearance>.05&&[-.51,0,.51].every(ox=>[-.51,0,.51].every(oy=>!blocked.has(tile({x:p.x+ox,y:p.y+oy})))))goals.push({p,clearance});
    }
    assert(goals.length,'No interaction point holds selection across ground flow '+JSON.stringify({target,other}));
    return goals.sort((a,b)=>b.clearance-a.clearance).map(({p})=>p);
  }
  const moving=new Set(defeated.view.tiles.filter(t=>push.get(t.ground)?.some(Boolean)).map(t=>t.y*map.width+t.x));
  async function select(p,target,other){
    const attempts=[];
    for(let n=0;n<28;n++){
      const v=await game.observe();
      if(dist(v.position,p)<.4&&margin(v.position,target,other)>.035)return true;
      // Alpha Square moves the character between inputs (data/grounds.xml:1028–1074).
      // Re-observe after each short native leg rather than precomputing a multi-leg press.
      const {points}=nativeRoute(map,v.position,p,{hazard:moving,reach:.1});
      const q=points.find(q=>dist(v.position,q)>.22)??p;
      const dx=q.x-v.position.x,dy=q.y-v.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await act({keys,duration_ms:Math.max(50,Math.min(220,Math.round(dist(v.position,q)*85)))});
      attempts.push({from:v.position,point:q});
    }
    await writeFile(join(out,`limon-route-miss-${p.x}-${p.y}.json`),JSON.stringify({p,target,other,attempts}));
    return false;
  }
  const slot=bag.items.indexOf(0xa4c),from={x:634+44*(slot%4),y:528+44*Math.floor(slot/4)};
  const before=await game.observe(),attempts=[];
  let picked=defeated;
  // A moving floor can replace the panel during the drag; try distinct approach points, never grant the item.
  for(const p of nearSide(bag.pos,portal.pos).slice(0,5)){
    if(!await select(p,bag.pos,portal.pos))continue;
    const start=await game.observe();
    assert(start.character.inventory_revision===before.character.inventory_revision,'Unexpected inventory commit before drag');
    await game.act({keys:[],duration_ms:230,drag:{from,to:{x:634,y:428}}});
    picked=await record();attempts.push({p,start:start.position,end:picked.view.position,revision:picked.view.character.inventory_revision});
    await writeFile(join(out,'limon-reward-attempts.json'),JSON.stringify(attempts));
    if(picked.view.character.inventory_revision!==before.character.inventory_revision)break;
  }
  game.check('native drag acquires eligible Dexterity once',picked.view.character.items[4]===0xa4c&&picked.view.character.inventory_revision===before.character.inventory_revision+1&&entity(picked,bag.id)?.items[slot]!==0xa4c);
  await game.act({keys:[],duration_ms:230,drag:{from,to:{x:678,y:428}}});game.check('repeated empty-slot drag cannot duplicate Dexterity',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  // The elements own their 20s Decay (actors.xml:5633,5658,5683,5708), even after Limon retires.
  const hasElements=s=>boxElements(s).length>0;
  let cleaned=picked;
  while(hasElements(cleaned)&&cleaned.diagnostic.time_ms-defeated.diagnostic.time_ms<20500){
    await protect();await game.act({keys:[],duration_ms:1000});cleaned=await record();
  }
  game.check('remaining box elements expire on their own without further births',!hasElements(cleaned));
  // The emptied bag still competes for the native panel until it expires.
  for(const p of nearSide(portal.pos,bag.pos).slice(0,5)){
    if(!await select(p,portal.pos,bag.pos))continue;
    await game.act({keys:[],mouse:{x:700,y:575},duration_ms:550});
    if((await game.observe()).session.world!==defeated.view.session.world)break;
  }
  const returned=await record();game.check('glowing exit returns the same living character with reward and once-only clear',returned.view.map.name==='Realm'&&returned.view.session.character===initial.view.session.character&&returned.view.lifecycle.alive&&returned.view.character.items[4]===0xa4c&&returned.diagnostic.character.dungeons[1]===defeated.diagnostic.character.dungeons[1]);await act({keys:[],duration_ms:700},{name:'limon-returned',when:()=>true});
}
