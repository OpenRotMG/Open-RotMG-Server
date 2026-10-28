import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

export const fixture={class:0x31e,level:20,seed:857,world:'sprite-world',worldSeed:1,position:world=>{const b=world.encounter_markers.find(e=>e.kind===0xd06);assert(b);return{x:b.position.x+1,y:b.position.y};},equipment:[0xb0b,-1,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const samples=[],shots=new Map(),corners=new Map();let protectedUntil=0;
  const dist=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y),entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  async function protect(){const now=(await game.observe()).game_ms;if(now<protectedUntil)return;const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}),signal:AbortSignal.timeout(10000)});const receipt=await r.json();assert(r.ok&&receipt.status==='succeeded',JSON.stringify(receipt));protectedUntil=now+60000;}
  async function record(){const s=await game.inspect(),{tiles,...view}=s.view;samples.push({...s,view});for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);for(const e of s.diagnostic.entities)if(e.kind>=0xd07&&e.kind<=0xd0a)corners.set(e.id,e);return s;}
  async function act(input,capture){await protect();let pending,error;const timer=setInterval(()=>{if(!pending)pending=record().catch(e=>{error=e;}).finally(()=>pending=null);},200);try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}if(error)throw error;const s=await record();await writeFile(join(out,'limon.json'),JSON.stringify({samples,shots:[...shots.values()],corners:[...corners.values()]}));return s;}
  async function walk(goal,range=.55){for(let n=0;n<50;n++){const v=await game.observe(),p=typeof goal==='number'?entity(await game.inspect(),goal)?.pos:goal;assert(p,'Walking target missing');const dx=p.x-v.position.x,dy=p.y-v.position.y;if(dist(v.position,p)<range)return;const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');await act({keys,duration_ms:Math.max(50,Math.min(300,Math.round(dist(v.position,p)/7.5*600)))});}throw Error('Native movement did not reach '+JSON.stringify(goal));}
  const initial=await record(),boss=initial.diagnostic.entities.find(e=>e.kind===0xd06);assert(boss);
  game.check('natural Limon starts with selected 9000 HP',boss.hp===9000);
  await act({keys:[],duration_ms:1000},{name:'limon-arrival',when:v=>v.lifecycle.alive});
  // The period guide establishes the phase sequence, not numeric timing: https://web.archive.org/web/20120609023618id_/http://forums.wildshadow.com/wiki/realm-mad-god/limon-sprite-god
  for(let n=0;n<40&&!samples.some(s=>entity(s,boss.id)?.stage%5===4);n++)await act({keys:[],duration_ms:1000});
  await act({keys:[],duration_ms:1500});
  game.check('native observer sees orbit box and summon phases',[2,3,4].every(stage=>samples.some(s=>entity(s,boss.id)?.stage%5===stage)));
  game.check('orbit moves Limon and publishes her selected Red Fire',samples.some(s=>entity(s,boss.id)?.stage%5===2&&dist(entity(s,boss.id).pos,boss.home)>.2)&&[...shots.values()].some(p=>p.owner===boss.id&&p.speed===80&&p.lifetime_ms===2300));
  const box=samples.find(s=>s.diagnostic.entities.filter(e=>e.parent===boss.id&&e.kind>=0xd07&&e.kind<=0xd0a).length===8);assert(box,'Eight natural box elements missing');
  const elements=box.diagnostic.entities.filter(e=>e.parent===boss.id&&e.kind>=0xd07&&e.kind<=0xd0a);
  game.check('box creates two typed invulnerable radial squares',elements.every(e=>e.hp===100000&&(e.condition&(1<<24)))&&[0xd07,0xd08,0xd09,0xd0a].every(k=>elements.filter(e=>e.kind===k).length===2)&&elements.filter(e=>Math.abs(dist(e.pos,boss.home)-9.5)<.01).length===4&&elements.filter(e=>Math.abs(dist(e.pos,boss.home)-14)<.01).length===4);
  game.check('box alternates shielded and vulnerable Limon',samples.some(s=>entity(s,boss.id)?.stage%5===3&&(entity(s,boss.id).condition&(1<<24)))&&samples.some(s=>entity(s,boss.id)?.stage%5===3&&!(entity(s,boss.id).condition&(1<<24))));
  game.check('elements fire selected White Flame',elements.some(e=>[...shots.values()].some(p=>p.owner===e.id&&p.speed===90&&p.lifetime_ms===1940)));
  game.check('elements expire and ordinary Ice and Magic Sprites appear',samples.some(s=>entity(s,boss.id)?.stage%5===4&&elements.every(e=>!entity(s,e.id)))&&[0x625,0x626].every(k=>samples.some(s=>s.diagnostic.entities.some(e=>e.parent===boss.id&&e.kind===k))));
  await walk(boss.id,2);await act({keys:[],duration_ms:900},{name:'limon-summons',when:()=>true});
  for(let n=0;n<100;n++){const s=await game.inspect(),b=entity(s,boss.id);if(!b)break;if(dist(s.view.position,b.pos)>2.5)await walk(boss.id,2);await act({keys:[],duration_ms:500,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});}
  const defeated=await record();
  game.check('native sword damage defeats Limon and grants one Sprite clear',!entity(defeated,boss.id)&&defeated.diagnostic.character.dungeons[1]===initial.diagnostic.character.dungeons[1]+1&&defeated.view.character.experience>initial.view.character.experience);
  game.check('defeat cleans up all owned box elements',!defeated.diagnostic.entities.some(e=>e.parent===boss.id&&e.kind>=0xd07&&e.kind<=0xd0a));
  const bag=defeated.diagnostic.entities.find(e=>e.items?.includes(0xa4c));assert(bag,'Eligible Dexterity reward missing');await walk(bag.pos);await act({keys:[],duration_ms:900},{name:'limon-reward',when:()=>true});
  const slot=bag.items.indexOf(0xa4c),before=await game.observe();await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},to:{x:634,y:428}}});
  const picked=await record();game.check('native drag acquires eligible Dexterity once',picked.view.character.items[4]===0xa4c&&picked.view.character.inventory_revision===before.character.inventory_revision+1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},to:{x:678,y:428}}});game.check('repeated empty-slot drag cannot duplicate Dexterity',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  const portal=picked.diagnostic.entities.find(e=>e.kind===0x70e);assert(portal);await walk(portal.pos);await act({keys:[],duration_ms:900},{name:'limon-exit',when:()=>true});await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const returned=await record();game.check('glowing exit returns the same living character with reward and once-only clear',returned.view.map.name==='Realm'&&returned.view.session.character===initial.view.session.character&&returned.view.lifecycle.alive&&returned.view.character.items[4]===0xa4c&&returned.diagnostic.character.dungeons[1]===defeated.diagnostic.character.dungeons[1]);await act({keys:[],duration_ms:700},{name:'limon-returned',when:()=>true});
}
