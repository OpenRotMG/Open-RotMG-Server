import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
export const fixture={class:0x30e,level:20,seed:935,world:'oryx-castle',position:{x:24.5,y:139.5},equipment:[0xb08,0xb24,-1,-1],backpack:[0xa23,0xc1a,0xc21,-1,-1,-1,-1,-1],stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const root=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(root,'native/native.json'),'utf8')),samples=[];
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  async function prepare(actor,change){
    const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:actor.actor,change}}),signal:AbortSignal.timeout(10000)});
    const value=await r.json();assert(r.ok&&value.status==='succeeded',JSON.stringify(value));
  }
  async function save(name,actor=game){const state=await actor.inspect();samples.push({name,actor:actor.actor,...state});await writeFile(join(root,'quiet-observations.json'),JSON.stringify(samples));return state;}
  const initial=await save('initial');
  await prepare(game,{kind:'protection',condition:'invulnerable',duration_ms:60000});
  await game.act({keys:[],duration_ms:1000},{name:'quiet-applied',when:v=>!!(v.character.conditions&2)});
  const hit=await save('natural-hit');
  game.check('natural selected five-second Living Floor hit drains MP through HP protection',!!(hit.view.character.conditions&2)&&hit.view.character.mp===0&&hit.view.character.hp===initial.view.character.hp);
  const peer=await game.join({...fixture,world:hit.view.session.world,position:hit.view.position,backpack:[0xc1a,-1,-1,-1,-1,-1,-1,-1]});
  await prepare(peer,{kind:'protection',condition:'invincible',duration_ms:60000});
  await prepare(peer,{kind:'vitals',hp:670,mp:10});
  await game.act({keys:['1'],duration_ms:200});
  const potion=await save('potion');
  // Kithio consumes Magic/MagicNova before its next Quiet drain; our committed suppression avoids that transient grant: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/entities/player/Player.Effects.cs#L27-L32
  game.check('Quiet wastes exactly one normal mana potion without a mana grant',!!(potion.view.character.conditions&2)&&potion.view.character.mp===0&&potion.view.character.items[4]===-1&&potion.view.character.inventory_revision===hit.view.character.inventory_revision+1);
  const peerBefore=await save('peer-before',peer);
  await game.act({keys:['2'],duration_ms:200},{name:'quiet-potions',when:v=>v.character.items[5]===-1});
  const nova=await save('quiet-caster-nova'),peerAfter=await save('peer-after',peer);
  game.check('Quiet caster consumes MagicNova and stays empty while eligible nearby peer gains mana',!!(nova.view.character.conditions&2)&&nova.view.character.mp===0&&nova.view.character.items[5]===-1&&nova.view.character.inventory_revision===potion.view.character.inventory_revision+1&&peerAfter.view.character.mp>=peerBefore.view.character.mp+100&&!(peerAfter.view.character.conditions&2));
  await peer.act({keys:['1'],duration_ms:200});
  const received=await save('quiet-recipient-nova'),giver=await save('giver',peer);
  game.check('unquiet peer MagicNova consumes once but cannot restore Quiet recipient',received.view.character.mp===0&&!!(received.view.character.conditions&2)&&giver.view.character.items[4]===-1&&giver.view.character.inventory_revision===peerAfter.view.character.inventory_revision+1);
  await game.act({keys:[' '],duration_ms:100});
  const refused=await save('refused');
  game.check('Quiet refuses equipped spell without cooldown or inventory mutation',refused.view.character.mp===0&&refused.view.character.ability_ready_ms<=refused.view.time_ms&&refused.view.character.inventory_revision===received.view.character.inventory_revision&&refused.diagnostic.character.shots===received.diagnostic.character.shots);
  const floor=refused.diagnostic.entities.filter(e=>e.kind===0xd87).sort((a,b)=>Math.hypot(a.pos.x-refused.view.position.x,a.pos.y-refused.view.position.y)-Math.hypot(b.pos.x-refused.view.position.x,b.pos.y-refused.view.position.y))[0];assert(floor);
  await prepare(game,{kind:'stat_modifier',index:2,amount:100,duration_ms:60000});
  for(let n=0;n<40;n++){
    if(!(await game.inspect()).diagnostic.entities.some(e=>e.id===floor.id))break;
    await game.act({keys:[],duration_ms:200,target:{entity:floor.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const killed=await save('source-defeated');
  game.check('Quiet permits ordinary weapon fire to defeat its actual source',!killed.diagnostic.entities.some(e=>e.id===floor.id)&&killed.diagnostic.character.kills>refused.diagnostic.character.kills&&!!(killed.view.character.conditions&2));
  await game.act({keys:['3'],duration_ms:200},{name:'quiet-cured',when:v=>!(v.character.conditions&2)});
  const cured=await save('cured');
  game.check('ordinary Holy Water consumes once and clears natural Quiet',!(cured.view.character.conditions&2)&&cured.view.character.items[6]===-1&&cured.view.character.inventory_revision===killed.view.character.inventory_revision+1);
  await prepare(peer,{kind:'item',slot:4,item:0xc1a});
  await peer.act({keys:[],duration_ms:100});await peer.act({keys:['1'],duration_ms:200});
  const restored=await save('restored');
  game.check('MagicNova restores mana after cure',!(restored.view.character.conditions&2)&&restored.view.character.mp>=100);
  await game.act({keys:[' '],duration_ms:100});
  const cast=await save('cast');
  game.check('normal spell works after cure and restoration',cast.view.character.ability_ready_ms>cast.view.time_ms&&cast.view.character.mp<restored.view.character.mp&&cast.diagnostic.character.shots>restored.diagnostic.character.shots);
  const before=await game.observe();await game.act({keys:['Insert'],duration_ms:500});const after=await save('nexus');
  game.check('Nexus transfer preserves living character and committed consumptions without Quiet',after.view.map.name==='Nexus'&&after.view.lifecycle.alive&&after.view.session.character===before.session.character&&JSON.stringify(after.view.character.items)===JSON.stringify(before.character.items)&&!(after.view.character.conditions&2));
}
