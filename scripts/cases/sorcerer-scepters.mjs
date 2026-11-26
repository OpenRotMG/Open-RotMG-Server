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
  let shielded=0;
  // Protection cannot be reapplied while active: finish its window idle, then renew it before the next action.
  async function act(input,capture){
    let now=(await game.observe()).game_ms;
    if(now<shielded&&shielded-now<input.duration_ms){await game.act({keys:[],duration_ms:Math.max(50,shielded-now)});now=(await game.observe()).game_ms;}
    if(now>=shielded){await prepare({kind:'protection',condition:'invincible',duration_ms:60000});shielded=now+60000;}
    return game.act(input,capture);
  }
  await act({keys:[],duration_ms:500});
  const start=await game.observe();
  // objects.xml Defense: Septavius 12, Lair Ghosts 15, and his XML MakeMinions Ghosts of Septavius (actors.xml:9852-9854) 20.
  const defense={0xd90:12,...Object.fromEntries([0xdaa,0xdab,0xdac,0xdad,0xdae,0xdaf].map(k=>[k,15])),...Object.fromEntries([0xdb0,0xdb1,0xdb2].map(k=>[k,20]))};
  let slowed=[];
  for(const [slot,item,name,cost,damage,maximum] of [[-1,0xae5,'lightning',60,50,3],[0,0xb33,'storms',85,200,8],[1,0xc30,'fulmination',65,180,7]]){
    if(slot>=0){
      await act({keys:[],duration_ms:350,drag:{from:{x:634+44*slot,y:428},to:{x:678,y:385}}});
      game.check(name+' equips through native inventory',(await game.observe()).character.items[1]===item);
    }
    // XML Septavius (actors.xml:9725) wanders and spends 5 of 8 Triforce substates Invulnerable; recast until a cast lands in a vulnerable window.
    let before,after,target,hits,prior;
    for(let attempt=0;attempt<8;attempt++){
      await vitals(380);
      // Storms needs a second enemy; XML Septavius only summons his Ghosts in Summon_the_undead (actors.xml:9849-9856).
      for(let n=0;item===0xb33&&n<150;n++){
        const s=await game.inspect(),b=s.diagnostic.entities.find(e=>e.id===boss.id);assert(b);
        if(s.diagnostic.entities.some(e=>e.id!==boss.id&&e.hp>0&&Object.hasOwn(defense,e.kind)&&Math.hypot(e.pos.x-b.pos.x,e.pos.y-b.pos.y)<6))break;
        await act({keys:[],duration_ms:500});
      }
      for(let n=0;n<30;n++){
        before=await snapshot(name+'-before');target=before.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
        const dx=target.pos.x-before.view.position.x,dy=target.pos.y-before.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
        if(Math.hypot(dx,dy)>3)await act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:150});
        else if(target.condition&(1<<24))await act({keys:[],duration_ms:100});
        else break;
      }
      game.check(name+' targets an observed vulnerable boss',!(target.condition&(1<<24)));
      await act({keys:[' '],target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:false},duration_ms:250},attempt===0?{name,when:v=>v.character.mp<before.view.character.mp-cost/2}:undefined);
      after=await snapshot(name+'-after');target=after.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
      prior=new Map(before.diagnostic.entities.map(e=>[e.id,e]));
      hits=after.diagnostic.entities.filter(e=>Object.hasOwn(e.damage,String(game.actor))&&e.damage[game.actor]>(prior.get(e.id)?.damage[game.actor]??0));
      if(hits.length>=(item===0xb33?2:1))break;
      await act({keys:[],duration_ms:600});
    }
    const regenerated=Math.ceil((after.view.time_ms-before.view.time_ms)/1000*(0.6+after.view.character.stats[7]/16.7));
    game.check(name+' spends one cost within observed regeneration',regenerated<cost&&after.view.character.mp>=before.view.character.mp-cost&&after.view.character.mp<=before.view.character.mp-cost+regenerated);
    game.check(name+' hits distinct enemies within its selected cap',hits.length>0&&hits.length<=maximum);
    game.check(name+' applies equal raw damage less each enemy defense',hits.every(e=>{assert(Object.hasOwn(defense,e.kind),'Unexpected lightning target '+e.kind);return e.damage[game.actor]-(prior.get(e.id)?.damage[game.actor]??0)===damage-defense[e.kind];}));
    if(item===0xb33)game.check('Storms chains between multiple observed enemies',hits.length>=2);
    if(item===0xc30){
      slowed=hits.map(e=>e.id);
      game.check('Fulmination applies Slowed for exactly three seconds to every observed hit',hits.every(e=>e.statuses[4][1]-e.statuses[4][0]===3000));
      game.check('Fulmination adds two attack and wisdom',after.view.character.stats[2]===62&&after.view.character.stats[7]===62);
    }
    await act({keys:[],duration_ms:600});
  }
  await act({keys:[],duration_ms:2500});
  const expired=await snapshot('expired');
  game.check('Slowed expires in every observed hit client-visible condition mask',slowed.every(id=>!(expired.view.objects.find(e=>e.id===id).condition_mask&(1<<3))));
  await vitals(0);const poor=await snapshot('poor');
  await act({keys:[' '],target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:false},duration_ms:250});
  const denied=await snapshot('denied');
  game.check('insufficient mana cannot add enemy damage credit',poor.diagnostic.entities.every(e=>JSON.stringify(e.damage)===JSON.stringify(denied.diagnostic.entities.find(q=>q.id===e.id).damage)));
  game.check('same living Sorcerer retains Fulmination',denied.view.lifecycle.alive&&denied.view.session.character===start.session.character&&denied.view.character.items[1]===0xc30);
}
