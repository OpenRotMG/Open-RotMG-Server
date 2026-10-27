import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const out=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x325,level:20,seed:775,world:'undead-lair',worldSeed:12351,position:'spawn',equipment:[-1,0xae5,-1,-1],backpack:[0xb33,0xc30,-1,-1,-1,-1,-1,-1],stats:[670,385,60,25,60,60,75,60]};
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const headers={Authorization:'Bearer '+token,'Content-Type':'application/json'};
  const initial=await game.inspect();
  const response=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers,signal:AbortSignal.timeout(10000)});assert(response.ok);
  const map=await response.json();
  await writeFile(join(out,'map.json'),JSON.stringify(map));
  const emptyBefore=await game.observe();
  game.check('prepared entry has no nearby enemy',!emptyBefore.nearby.some(e=>e.hp>0&&e.name==='Septavius the Ghost God'));
  await game.act({keys:[' '],duration_ms:250},{name:'empty-cast',when:v=>v.character.mp<emptyBefore.character.mp-30});
  const emptyAfter=await game.observe();
  game.check('empty cast still spends the Lightning Scepter cost',emptyBefore.character.mp-emptyAfter.character.mp>=58&&emptyBefore.character.mp-emptyAfter.character.mp<=60);
  const boss=map.entities.find(e=>e.kind===0xd90);assert(boss);
  const {worldSeed,...sorcerer}=fixture;
  game=await game.join({...sorcerer,world:initial.view.session.world,position:{x:boss.pos.x-2,y:boss.pos.y}});
  const records=[];
  async function snapshot(label){const data=await game.inspect();records.push({label,...data});await writeFile(join(out,'scepter-observations.json'),JSON.stringify(records,null,2));return data;}
  async function vitals(mp){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'vitals',hp:670,mp}}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  await game.act({keys:[],duration_ms:500});
  const start=await game.observe();
  for(const [slot,item,name,cost,damage] of [[-1,0xae5,'lightning',60,50],[0,0xb33,'storms',85,200],[1,0xc30,'fulmination',65,180]]){
    if(slot>=0){
      await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*slot,y:428},to:{x:678,y:385}}});
      game.check(name+' equips through native inventory',(await game.observe()).character.items[1]===item);
    }
    await vitals(380);
    const before=await snapshot(name+'-before'),targetBefore=before.diagnostic.entities.find(e=>e.id===boss.id);assert(targetBefore);
    await game.act({keys:[' '],target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:false},duration_ms:250},{name,when:v=>v.character.mp<before.view.character.mp-cost/2});
    const after=await snapshot(name+'-after'),target=after.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
    game.check(name+' spends one selected mana cost',after.view.character.mp>=before.view.character.mp-cost&&after.view.character.mp<before.view.character.mp-cost+5);
    game.check(name+' damages the observed enemy',targetBefore.hp-target.hp===damage-12);
    if(item===0xc30){
      game.check('Fulmination applies Slowed for exactly three seconds',target.statuses[4][1]-target.statuses[4][0]===3000);
      game.check('Fulmination adds two attack and wisdom',after.view.character.stats[2]===62&&after.view.character.stats[7]===62);
    }
    await game.act({keys:[],duration_ms:600});
  }
  await game.act({keys:[],duration_ms:2500});
  const expired=await snapshot('expired');
  game.check('Slowed expires in the client-visible condition mask',!(expired.view.objects.find(e=>e.id===boss.id).condition_mask&(1<<3)));
  await vitals(0);const poor=await snapshot('poor');
  await game.act({keys:[' '],target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:false},duration_ms:250});
  const denied=await snapshot('denied');
  game.check('insufficient mana cannot damage the enemy',denied.diagnostic.entities.find(e=>e.id===boss.id).hp===poor.diagnostic.entities.find(e=>e.id===boss.id).hp);
  game.check('same living Sorcerer retains Fulmination',denied.view.lifecycle.alive&&denied.view.session.character===start.session.character&&denied.view.character.items[1]===0xc30);
}
