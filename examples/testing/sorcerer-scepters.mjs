import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const out=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x325,level:20,seed:775,world:'undead-lair',worldSeed:12351,position(info){const [x,y]=info.spawn_points[0];return{x:x+4,y:y-4};},equipment:[-1,0xae5,-1,-1],backpack:[0xb33,0xc30,-1,-1,-1,-1,-1,-1],stats:[670,385,60,25,60,60,75,60]};
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const headers={Authorization:'Bearer '+token,'Content-Type':'application/json'};
  const initial=await game.inspect();
  const response=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers,signal:AbortSignal.timeout(10000)});assert(response.ok);
  const map=await response.json();
  await writeFile(join(out,'map.json'),JSON.stringify(map));
  const emptyBefore=await game.observe();
  game.check('prepared entry has no live entity in lightning reach',map.entities.every(e=>e.hp<=0||Math.hypot(e.pos.x-emptyBefore.position.x,e.pos.y-emptyBefore.position.y)>14));
  await game.act({keys:[' '],duration_ms:250},{name:'empty-cast',when:v=>v.character.mp<emptyBefore.character.mp-30});
  const emptyAfter=await game.observe();
  const emptyRegen=Math.ceil((emptyAfter.game_ms-emptyBefore.game_ms)/1000*(0.6+emptyAfter.character.stats.wisdom/16.7));
  game.check('empty cast spends one cost within observed regeneration',emptyRegen<60&&emptyBefore.character.mp-emptyAfter.character.mp>=60-emptyRegen&&emptyBefore.character.mp-emptyAfter.character.mp<=60);
  await prepare({kind:'protection',condition:'invincible',duration_ms:60000});
  const boss=map.entities.find(e=>e.kind===0xd90);assert(boss);
  const {worldSeed,...sorcerer}=fixture;
  game=await game.join({...sorcerer,world:initial.view.session.world,position:{x:boss.pos.x-2,y:boss.pos.y}});
  const records=[];
  async function snapshot(label){const data=await game.inspect();records.push({label,...data});await writeFile(join(out,'scepter-observations.json'),JSON.stringify(records,null,2));return data;}
  async function prepare(change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  const vitals=mp=>prepare({kind:'vitals',hp:670,mp});
  await prepare({kind:'protection',condition:'invincible',duration_ms:60000});
  await game.act({keys:[],duration_ms:500});
  const start=await game.observe();
  let slowed=[];
  for(const [slot,item,name,cost,damage,maximum] of [[-1,0xae5,'lightning',60,50,3],[0,0xb33,'storms',85,200,8],[1,0xc30,'fulmination',65,180,7]]){
    if(slot>=0){
      await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*slot,y:428},to:{x:678,y:385}}});
      game.check(name+' equips through native inventory',(await game.observe()).character.items[1]===item);
    }
    await vitals(380);
    let before;
    for(let n=0;n<8;n++){
      before=await snapshot(name+'-before');
      const target=before.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
      if(!(target.condition&(1<<24)))break;
      await game.act({keys:[],duration_ms:500});
    }
    game.check(name+' targets an observed vulnerable boss',!(before.diagnostic.entities.find(e=>e.id===boss.id).condition&(1<<24)));
    await game.act({keys:[' '],target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:false},duration_ms:250},{name,when:v=>v.character.mp<before.view.character.mp-cost/2});
    const after=await snapshot(name+'-after'),target=after.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
    const regenerated=Math.ceil((after.view.time_ms-before.view.time_ms)/1000*(0.6+after.view.character.stats[7]/16.7));
    game.check(name+' spends one cost within observed regeneration',regenerated<cost&&after.view.character.mp>=before.view.character.mp-cost&&after.view.character.mp<=before.view.character.mp-cost+regenerated);
    const prior=new Map(before.diagnostic.entities.map(e=>[e.id,e]));
    const hits=after.diagnostic.entities.filter(e=>Object.hasOwn(e.damage,String(game.actor))&&e.damage[game.actor]>(prior.get(e.id)?.damage[game.actor]??0));
    game.check(name+' hits distinct enemies within its selected cap',hits.length>0&&hits.length<=maximum);
    game.check(name+' applies equal raw damage less each enemy defense',hits.every(e=>{assert(prior.has(e.id));assert(e.kind===0xd90||(e.kind>=0xdaa&&e.kind<=0xdaf));return e.damage[game.actor]-(prior.get(e.id).damage[game.actor]??0)===damage-(e.kind===0xd90?12:15);}));
    if(item===0xb33)game.check('Storms chains between multiple observed enemies',hits.length>=2);
    if(item===0xc30){
      slowed=hits.map(e=>e.id);
      game.check('Fulmination applies Slowed for exactly three seconds to every observed hit',hits.every(e=>e.statuses[4][1]-e.statuses[4][0]===3000));
      game.check('Fulmination adds two attack and wisdom',after.view.character.stats[2]===62&&after.view.character.stats[7]===62);
    }
    await game.act({keys:[],duration_ms:600});
  }
  await game.act({keys:[],duration_ms:2500});
  const expired=await snapshot('expired');
  game.check('Slowed expires in every observed hit client-visible condition mask',slowed.every(id=>!(expired.view.objects.find(e=>e.id===id).condition_mask&(1<<3))));
  await vitals(0);const poor=await snapshot('poor');
  await game.act({keys:[' '],target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:false},duration_ms:250});
  const denied=await snapshot('denied');
  game.check('insufficient mana cannot add enemy damage credit',poor.diagnostic.entities.every(e=>JSON.stringify(e.damage)===JSON.stringify(denied.diagnostic.entities.find(q=>q.id===e.id).damage)));
  game.check('same living Sorcerer retains Fulmination',denied.view.lifecycle.alive&&denied.view.session.character===start.session.character&&denied.view.character.items[1]===0xc30);
}
