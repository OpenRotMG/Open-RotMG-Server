import assert from 'node:assert/strict';
import {appendFile,readFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

export const fixture={class:0x300,level:20,seed:868,world:'wine-cellar',position:w=>{const b=w.encounter_markers.find(e=>e.kind===0x932);assert(b);return{x:b.position.x-.75,y:b.position.y};},equipment:[0xaff,0xb27,0xaf9,0xacd],backpack:Array(8).fill(-1),stats:[720,252,50,25,75,75,40,50]};
export async function run(game){
 const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
 const samples=[];let protectedUntil=0,protectedWorld,rageObserved=false;
 const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y),find=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
 async function record(label){const s=await game.inspect(),{tiles,...view}=s.view;samples.push({...s,view});await appendFile(join(out,'wine-cellar.jsonl'),JSON.stringify({label,...s,view})+'\n');return s;}
 async function act(input,capture){let v=await game.observe();if(v.session.world!==protectedWorld){protectedWorld=v.session.world;protectedUntil=v.game_ms;}if(protectedUntil>v.game_ms&&protectedUntil-v.game_ms<4000&&!(v.character.conditions&(1<<12))){assert(v.character.mp>=85&&v.game_ms>=v.character.ability_ready_ms,'Cloak must cover the protection-expiry boundary');await game.act({keys:[' '],duration_ms:150});v=await game.observe();}if(v.game_ms>=protectedUntil){const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}}),signal:AbortSignal.timeout(10000)});const result=await r.json();assert(r.ok&&result.status==='succeeded',JSON.stringify(result));protectedUntil=v.game_ms+60000;}const remaining=protectedUntil-v.game_ms;if(input.duration_ms>remaining)input={...input,duration_ms:remaining};await game.act(input,capture);return record('action');}
 async function walk(goal,range=.5){for(let n=0;n<80;n++){const s=await game.inspect(),p=typeof goal==='number'?find(s,goal)?.pos:goal;assert(p,'Native walking target missing');const a=s.view.position,d=distance(a,p);if(d<range)return;const x=p.x-a.x-p.y+a.y,y=p.x-a.x+p.y-a.y,keys=[];if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');await act({keys,duration_ms:Math.max(50,Math.min(300,Math.round(d*80)))});}assert.fail('Native route blocked '+JSON.stringify(goal));}
 const initial=await record('initial'),boss=initial.diagnostic.entities.find(e=>e.kind===0x932);assert(boss);
 game.check('natural Oryx 2 starts at selected 75000 HP',boss.hp===75000);
 await act({keys:[],duration_ms:1200});
 await act({keys:[],duration_ms:5000});
 game.check('opening naturally summons Henchmen and selected wine minions',[0x944,0xdf1,0xdf2].every(k=>samples.some(s=>s.diagnostic.entities.some(e=>e.kind===k))));
 // The 2013 comparator differs from later .2/10s reconstruction; this verifies current mechanics, not historical fidelity: https://github.com/trapped/rotmg_svr/blob/593e490803fe0ad168ead408cacd5287d778acfe/wServer/logic/db/BehaviorDb.Oryx2.cs
 for(let n=0;n<200;n++){
  const s=await game.inspect(),b=find(s,boss.id);if(!b)break;
  if(!rageObserved&&(b.condition&(1<<24))){await record('shield-start');await walk({x:b.pos.x-7,y:b.pos.y});await act({keys:[],duration_ms:900});for(let i=0;i<12;i++)await act({keys:[],duration_ms:1000});const chasing=await record('rage-chase');game.check('rage actually moves Oryx 2 toward the retreating native player',distance(find(chasing,boss.id).pos,b.pos)>1&&!(find(chasing,boss.id).condition&(1<<24)));rageObserved=true;continue;}
  if(distance(s.view.position,b.pos)>1){await walk(boss.id,.75);continue;}
  await act({keys:[],duration_ms:2000,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
 }
 const defeated=await record('defeated');
 game.check('ordinary native dagger damage defeats Oryx 2 and credits once',!find(defeated,boss.id)&&defeated.diagnostic.character.lifetime.oryx_kills===initial.diagnostic.character.lifetime.oryx_kills+1&&defeated.view.character.experience>initial.view.character.experience);
 const shielded=samples.filter(s=>{const b=find(s,boss.id);return b&&(b.condition&(1<<24));});
 game.check('Oryx 2 shields at low HP then resumes vulnerable attacks',rageObserved&&shielded.length>0&&shielded.every(s=>find(s,boss.id).hp<=15000)&&samples.some(s=>s.view.time_ms>=shielded[0].view.time_ms+10000&&find(s,boss.id)&&!(find(s,boss.id).condition&(1<<24))));
 game.check('Oryx 2 publishes the selected slow stars blades and transition spinners',[70,100,90].every(speed=>samples.some(s=>s.view.projectiles.some(p=>p.owner===boss.id&&p.speed===speed))));
 const bag=defeated.diagnostic.entities.find(e=>e.items?.includes(0xa34));assert(bag,'Eligible Vitality reward missing');await walk(bag.pos);await act({keys:[],duration_ms:900});
 const slot=bag.items.indexOf(0xa34),before=await game.observe(),from={x:634+44*(slot%4),y:528+44*Math.floor(slot/4)};
 await act({keys:[],duration_ms:350,drag:{from,to:{x:634,y:428}}});const picked=await record('picked');
 game.check('native drag collects eligible Vitality exactly once',picked.view.character.items[4]===0xa34&&picked.view.character.inventory_revision===before.character.inventory_revision+1);
 await act({keys:[],duration_ms:350,drag:{from,to:{x:678,y:428}}});game.check('repeating empty-slot drag does not duplicate reward',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
 const exit=picked.diagnostic.entities.find(e=>e.kind===0x712&&e.published);assert(exit);await walk(exit.pos);await act({keys:[],duration_ms:900});await act({keys:[],duration_ms:1700,mouse:{x:700,y:575}});
 const returned=await record('returned');game.check('completion exit returns the same living character with reward and one Oryx kill',returned.view.map.name==='Nexus'&&returned.view.session.character===initial.view.session.character&&returned.view.lifecycle.alive&&returned.view.character.items[4]===0xa34&&returned.diagnostic.character.lifetime.oryx_kills===defeated.diagnostic.character.lifetime.oryx_kills);
 await act({keys:[],duration_ms:3000},{name:'oryx-two-returned',when:()=>true});
}
