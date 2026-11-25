import assert from 'node:assert/strict';
import {nativeMap} from './native-map.mjs';
import {appendFile,readFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

export const fixture={class:0x300,level:20,seed:868,world:'wine-cellar',position:w=>{const b=w.encounter_markers.find(e=>e.kind===0x932);assert(b);return{x:b.position.x-.75,y:b.position.y};},equipment:[0xaff,0xb27,0xaf9,0xacd],backpack:Array(8).fill(-1),stats:[720,252,50,25,75,75,40,50]};
export async function run(game){
 const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
 const samples=[];let protectedUntil=0,armoredUntil=0,protectedWorld;
 const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y),find=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
 async function record(label){const s=await game.inspect(),{tiles,...view}=s.view;samples.push({...s,view});await appendFile(join(out,'wine-cellar.jsonl'),JSON.stringify({label,...s,view})+'\n');return s;}
 const cheat=async change=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}}),signal:AbortSignal.timeout(10000)});const result=await r.json();assert(r.ok&&result.status==='succeeded',JSON.stringify(result));};
 // Protection cannot be renewed early: cloak and back away from XML Oryx 2's volleys across each renewal, under a staggered DEF window.
 // After defeat, renewing near a loot bag must not send retreat movement.
 let bossId;
 async function crossRenewal(v){const s=await game.inspect(),b=bossId&&find(s,bossId),remaining=protectedUntil-v.game_ms,steps=[];if(b&&v.character.mp>=85&&v.game_ms>=v.character.ability_ready_ms)steps.push({actor:game.actor,input:{keys:[' '],duration_ms:Math.min(150,remaining)}});const keys=[];if(b){const dx=v.position.x-b.pos.x,dy=v.position.y-b.pos.y,x=dx-dy,y=dx+dy;if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');}const back=Math.min(600,Math.max(1,remaining-150));steps.push({actor:game.actor,input:{keys,duration_ms:back}},{actor:game.actor,input:{keys:[],duration_ms:Math.max(1,remaining-150-back)+300}});await game.act(steps);}
 async function act(input,capture){let v=await game.observe();if(v.session.world===protectedWorld&&protectedUntil>v.game_ms&&protectedUntil-v.game_ms<2500){await crossRenewal(v);v=await game.observe();}if(v.session.world!==protectedWorld){protectedWorld=v.session.world;protectedUntil=armoredUntil=v.game_ms;}if(v.game_ms>=protectedUntil){await cheat({kind:'protection',condition:'invincible',duration_ms:60000});protectedUntil=v.game_ms+60000;}if(protectedUntil-v.game_ms<=15000&&v.game_ms>=armoredUntil){await cheat({kind:'stat_modifier',index:3,amount:100,duration_ms:60000});armoredUntil=v.game_ms+60000;}const usable=protectedUntil-v.game_ms-2400;if(Array.isArray(input)){let left=usable;const fits=input.filter(step=>(left-=step.input.duration_ms)>=0);input=fits.length?fits:[{...input[0],input:{...input[0].input,duration_ms:usable}}];}else if(input.duration_ms>usable)input={...input,duration_ms:usable};await game.act(input,capture);return record('action');}
 async function walk(goal,range=.5){for(let n=0;n<80;n++){const s=await game.inspect(),p=typeof goal==='number'?find(s,goal)?.pos:goal;assert(p,'Native walking target missing');const a=s.view.position,d=distance(a,p);if(d<range)return;const x=p.x-a.x-p.y+a.y,y=p.x-a.x+p.y-a.y,keys=[];if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');await act({keys,duration_ms:Math.max(50,Math.min(300,Math.round(d*80)))});}assert.fail('Native route blocked '+JSON.stringify(goal));}
 const initial=await record('initial'),boss=initial.diagnostic.entities.find(e=>e.kind===0x932);assert(boss);bossId=boss.id;
 game.check('natural Oryx 2 starts at selected 75000 HP',boss.hp===75000);
 await act({keys:[],duration_ms:1200});
 await act({keys:[],duration_ms:8000});
 // Each XML Henchman picks a random Group1-4 whose MakeMinions start after cooldown 7 (actors.xml:4553-4580), so Bile/Vintner are chance, not guaranteed.
 game.check('opening summons Henchmen whose random XML groups raise Oryx minions',samples.some(s=>s.diagnostic.entities.some(e=>e.kind===0x944))&&samples.some(s=>s.diagnostic.entities.some(e=>[0xde0,0xde1,0xde3,0xdf1,0xdf2].includes(e.kind))));
 // Recovered Oryx 2 (actors.xml:4121): minions until HP<30%, invulnerable ByeByeMinions 3s orders Henchmen to Decay, invulnerable TimeToDie1 7s spinner rings, then vulnerable TimeToDie2 to death.
 const states=(s,id)=>(s.diagnostic.xml[id]?.states??[]).map(x=>x.name),phase=s=>['ByeByeMinions','TimeToDie1','TimeToDie2'].find(n=>states(s,boss.id).includes(n))??'minions';
 const invulnerable=e=>Boolean(e.condition&(1<<24));
 // ByeByeMinions Orders listening Henchmen to Decay; count them world-wide, beyond the inspected neighborhood.
 const henchmen=async()=>(await nativeMap(config.origin,{Authorization:'Bearer '+token},(await game.observe()).session.player)).entities.filter(e=>e.kind===0x944).length,counts=[];
 for(let n=0;n<120;n++){
  const s=await game.inspect(),b=find(s,boss.id);if(!b)break;counts.push({phase:phase(s),henchmen:await henchmen()});
  // The 5.6-tile dagger (speed 140, 400 ms) reaches a strafing TimeToDie2 Oryx without hugging it.
  if(distance(s.view.position,b.pos)>3.5){await walk(boss.id,2.5);continue;}
  // Long fixed-aim bursts keep the fight inside the native receipt budget; short ones near the phase change and while TimeToDie2 strafes.
  const steps=phase(s)==='minions'&&b.hp>32000?6:1;
  await act(Array.from({length:steps},()=>({actor:game.actor,input:{keys:[],duration_ms:1000,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}}})));
 }
 const defeated=await record('defeated');
 game.check('ordinary native dagger damage defeats Oryx 2 and credits once',!find(defeated,boss.id)&&defeated.diagnostic.character.lifetime.oryx_kills===initial.diagnostic.character.lifetime.oryx_kills+1&&defeated.view.character.experience>initial.view.character.experience);
 const living=samples.filter(s=>find(s,boss.id)),of=name=>living.filter(s=>phase(s)===name);
 game.check('Oryx 2 stays vulnerable with minions until thirty percent HP',of('minions').every(s=>!invulnerable(find(s,boss.id)))&&of('ByeByeMinions').concat(of('TimeToDie1')).every(s=>find(s,boss.id).hp<=22500));
 game.check('ByeByeMinions and TimeToDie1 are invulnerable and ordered Henchmen decay',of('ByeByeMinions').length>0&&of('TimeToDie1').length>0&&of('ByeByeMinions').concat(of('TimeToDie1')).every(s=>invulnerable(find(s,boss.id)))&&counts.find(c=>c.phase.startsWith('TimeToDie')).henchmen<counts.filter(c=>c.phase==='minions').at(-1).henchmen);
 game.check('TimeToDie2 resumes vulnerable attacks until defeat',of('TimeToDie2').length>1&&of('TimeToDie2').every(s=>!invulnerable(find(s,boss.id)))&&find(of('TimeToDie2').at(-1),boss.id).hp<find(of('TimeToDie2')[0],boss.id).hp);
 game.check('Oryx 2 publishes the selected slow stars blades and TimeToDie spinners',[70,100,90].every(speed=>samples.some(s=>s.view.projectiles.some(p=>p.owner===boss.id&&p.speed===speed))));
 const bag=defeated.diagnostic.entities.find(e=>e.items?.includes(0xa34));assert(bag,'Eligible Vitality reward missing');await walk(bag.pos);await act({keys:[],duration_ms:900});
 const slot=bag.items.indexOf(0xa34),before=await game.observe(),from={x:634+44*(slot%4),y:528+44*Math.floor(slot/4)};
 await act({keys:[],duration_ms:350,drag:{from,to:{x:634,y:428}}});const picked=await record('picked');
 game.check('native drag collects eligible Vitality exactly once',picked.view.character.items[4]===0xa34&&picked.view.character.inventory_revision===before.character.inventory_revision+1);
 await act({keys:[],duration_ms:350,drag:{from,to:{x:678,y:428}}});game.check('repeating empty-slot drag does not duplicate reward',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
 // Recovered VictoryAtDeath publishes Portal to Nexus (src/world/xml.rs xml_victory_birth).
 const exit=picked.diagnostic.entities.find(e=>e.kind===0x71d&&e.published);assert(exit,'Oryx 2 VictoryAtDeath exit missing');
 // The portal sits 0.2 tiles off the earned bag; empty the bag so the client offers the portal instead of the bag.
 for(let n=0;n<8;n++){const s=await game.inspect(),b=find(s,bag.id);if(!b)break;const i=b.items.findIndex(x=>x>=0),free=s.view.character.items.findIndex((x,k)=>k>=4&&x===-1);assert(i>=0&&free>=4,'No backpack room for the earned bag');await act({keys:[],duration_ms:350,drag:{from:{x:634+44*(i%4),y:528+44*Math.floor(i/4)},to:{x:634+44*((free-4)%4),y:428+44*Math.floor((free-4)/4)}}});}
 assert(!find(await game.inspect(),bag.id),'Earned bag did not empty');
 await walk(exit.pos);await act({keys:[],duration_ms:900});await act({keys:[],duration_ms:1700,mouse:{x:700,y:575}});
 const returned=await record('returned');game.check('completion exit returns the same living character with reward and one Oryx kill',returned.view.map.name==='Nexus'&&returned.view.session.character===initial.view.session.character&&returned.view.lifecycle.alive&&returned.view.character.items[4]===0xa34&&returned.diagnostic.character.lifetime.oryx_kills===defeated.diagnostic.character.lifetime.oryx_kills);
 await act({keys:[],duration_ms:3000},{name:'oryx-two-returned',when:()=>true});
}
