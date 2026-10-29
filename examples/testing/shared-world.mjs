import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
export const fixture={class:0x30e,level:1,seed:917,world:-1,position:{x:12.5,y:8.5},equipment:[0xa97,-1,-1,-1],backpack:[0xb0c,-1,-1,-1,-1,-1,-1,-1],stats:null};
export async function run(game){
  const peer=await game.join({...fixture,position:{x:13.5,y:8.5},backpack:Array(8).fill(-1)}),records=[];
  async function save(label){const pair=await Promise.all([game.inspect(),peer.inspect()]);records.push({label,pair});await writeFile(join(process.env.QA_RESULTS_DIR,'native/shared-world.json'),JSON.stringify(records));return pair;}
  async function shot(name,g=game){const start=(await g.observe()).game_ms;await g.act({keys:[],duration_ms:1000},{name,when:v=>v.game_ms>=start+500});}
  async function walk(g,p){for(let n=0;n<40;n++){const v=await g.observe(),dx=p.x-v.position.x,dy=p.y-v.position.y,d=Math.hypot(dx,dy);if(d<.4)return;const x=dx-dy,y=dx+dy,keys=[];if(Math.abs(x)>.12)keys.push(x>0?'d':'a');if(Math.abs(y)>.12)keys.push(y>0?'s':'w');await g.act({keys,duration_ms:Math.min(350,Math.max(50,Math.round(d*130)))});}assert.fail('Native walk did not reach '+JSON.stringify(p));}
  const initial=await save('initial'),starts=initial.map(s=>s.view.session);
  game.check('two distinct owned living accounts share one Tutorial',starts[0].account!==starts[1].account&&starts[0].world===starts[1].world&&initial.every(s=>s.view.lifecycle.alive));
  await shot('shared-before');await shot('shared-peer-before',peer);
  await walk(game,{x:10.5,y:8.5});await walk(peer,{x:14.5,y:8.5});
  const moved=await save('mutual-movement');
  for(let i=0;i<2;i++){const other=moved[1-i].view;game.check('participant '+i+' observes the other moving at its authoritative position',moved[i].view.objects.some(o=>o.id===other.session.player&&Math.hypot(o.position.x-other.position.x,o.position.y-other.position.y)<.01));}
  let seen,complete=false;
  const firing=peer.act([{actor:game.actor,input:{keys:[],duration_ms:900,mouse:{x:450,y:300}}}],{name:'shared-ally-fire',when:v=>seen&&v.game_ms>=seen.view.time_ms+100}).finally(()=>complete=true);
  const watching=(async()=>{while(!complete){const s=await peer.inspect();if(s.view.projectiles.some(p=>p.owner===starts[0].player)){seen=s;return;}await delay(30);}assert.fail('No live ally projectile observed during native fire');})();
  for(const result of await Promise.allSettled([firing,watching]))if(result.status==='rejected')throw result.reason;
  await writeFile(join(process.env.QA_RESULTS_DIR,'native/ally-projectiles.json'),JSON.stringify(seen));await save('ally-fire');
  game.check('observer receives the other players live projectiles',!!seen);
  await game.act({keys:[],duration_ms:400,drag:{from:{x:634,y:428},to:{x:300,y:300}}});
  const dropped=await save('private-drop'),privateBag=dropped[0].diagnostic.entities.find(e=>e.items?.includes(0xb0c));assert(privateBag);
  game.check('supplied Soulbound drop is hidden from the other account',dropped[0].view.character.items[4]===-1&&dropped[0].view.objects.some(o=>o.id===privateBag.id)&&!dropped[1].view.objects.some(o=>o.id===privateBag.id));
  const hen=dropped[0].diagnostic.entities.find(e=>e.kind===0x6b2);assert(hen);
  game.check('both participants initially see the same authored enemy',dropped.every(s=>s.view.objects.some(o=>o.id===hen.id&&o.hp===hen.hp)));
  await walk(game,{x:12.5,y:8.5});
  for(let n=0;n<20&&(await game.inspect()).diagnostic.entities.some(e=>e.id===hen.id);n++)await game.act({keys:[],duration_ms:400,target:{entity:hen.id,camera_angle:-Math.PI/4,fire:true}});
  await peer.act({keys:[],duration_ms:500});const killed=await save('shared-kill');
  const publicBag=killed[0].diagnostic.entities.find(e=>e.kind===0x500&&e.items?.includes(0xab0));assert(publicBag);
  game.check('ordinary staff kill removes the same enemy for both viewers',killed.every(s=>!s.view.objects.some(o=>o.id===hen.id))&&!killed[0].diagnostic.entities.some(e=>e.id===hen.id));
  game.check('both viewers see the single earned public reward',killed.every(s=>s.view.objects.some(o=>o.id===publicBag.id)));
  await shot('shared-kill-observer',peer);await walk(peer,publicBag.pos);await shot('shared-public-loot',peer);
  const collecting=await peer.observe();await peer.act({keys:[],duration_ms:400,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  const picked=await peer.observe();game.check('noncontributor collects the public potion once',picked.character.items[4]===0xab0&&picked.character.inventory_revision===collecting.character.inventory_revision+1);
  await peer.act({keys:[],duration_ms:400,drag:{from:{x:634,y:528},to:{x:678,y:428}}});
  game.check('retrying the empty reward cannot duplicate it',(await peer.observe()).character.inventory_revision===picked.character.inventory_revision);
  // World-wide identity must end at departure, not at the combat radius: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/entities/player/Player.Update.cs#L22-L65
  await game.act({keys:['Insert'],duration_ms:1800});await peer.act({keys:[],duration_ms:500});const departed=await save('one-departed');await shot('shared-departure',peer);
  game.check('departure removes the old player from both object and marker audiences',departed[0].view.map.name==='Nexus'&&departed[1].view.map.name==='Tutorial'&&departed.every((s,i)=>!s.view.objects.some(o=>o.account===starts[1-i].account)&&!s.view.player_markers.some(o=>o.account===starts[1-i].account)));
  await peer.act({keys:['Insert'],duration_ms:1800});await game.act({keys:[],duration_ms:500});const reunited=await save('reunited');await shot('shared-reconnected');await shot('shared-peer-reconnected',peer);
  for(let i=0;i<2;i++){const s=reunited[i].view,other=reunited[1-i].view;game.check('participant '+i+' reconnects alive with the same account and character under a fresh player ID',s.map.name==='Nexus'&&s.lifecycle.alive&&s.session.account===starts[i].account&&s.session.character===starts[i].character&&s.session.player!==starts[i].player);game.check('participant '+i+' sees only the peers fresh world identity',s.objects.some(o=>o.id===other.session.player)&&!s.objects.some(o=>o.id===starts[1-i].player));}
  game.check('reconnect preserves the one collected potion and its revision',reunited[1].view.character.items[4]===0xab0&&reunited[1].view.character.inventory_revision===picked.character.inventory_revision&&reunited[0].view.character.items[4]===-1);
}
