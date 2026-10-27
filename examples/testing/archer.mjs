import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x307,level:20,seed:764,world:'undead-lair',worldSeed:12351,position:'spawn',equipment:[-1,0xa61,-1,-1],backpack:[0xade,0xb28,0xc07,0xa23,0xa23,0xa0d,0xa1a,0xa23],stats:[700,100,75,25,50,50,40,50]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const initial=await game.inspect();
  const response=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers:{Authorization:'Bearer '+token},signal:AbortSignal.timeout(10000)});assert(response.ok);
  const boss=(await response.json()).entities.find(e=>e.kind===0xd90);assert(boss);
  const {worldSeed,...archer}=fixture;
  game=await game.join({...archer,world:initial.view.session.world,position:{x:boss.pos.x-2,y:boss.pos.y}});
  const protection=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'archer-incoming-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}),signal:AbortSignal.timeout(10000)});
  const protectedState=await protection.json();assert(protection.ok&&protectedState.status!=='failed',JSON.stringify(protectedState));
  const observations=[{incomingProtection:protectedState}];
  const save=()=>writeFile(join(root,'archer-observations.json'),JSON.stringify(observations,null,2));
  const target=state=>state.diagnostic.entities.find(e=>e.kind===0xd90);
  const aim=state=>({x:300+(target(state).pos.x-state.view.position.x)*50,y:300+(target(state).pos.y-state.view.position.y)*50});
  const vulnerable=async()=>{
    for(let attempt=0;attempt<16;attempt++){
      const state=await game.inspect(),enemy=target(state);assert(enemy&&Number.isInteger(enemy.activated));
      const phase=(state.view.time_ms-enemy.activated)%13000;
      if(phase>=3500&&phase<10000)return state;
      await game.act({keys:[],duration_ms:500});
    }
    throw Error('No bounded Septavius vulnerability window observed');
  };
  await game.act({keys:[],duration_ms:3500});
  await game.act({keys:[],duration_ms:350,drag:{from:{x:678,y:472},to:{x:722,y:384}}});
  const armored=await game.observe();
  game.check('native leather armor equips and grants defense',armored.character.items[2]===0xa0d&&armored.character.stats.defense===31);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:722,y:472},to:{x:634,y:384}}});
  const bow=await vulnerable();
  game.check('native Archer equips a bow',bow.view.character.items[0]===0xa1a);
  await game.act({keys:[],mouse:aim(bow),duration_ms:700});
  const fired=await game.inspect();observations.push({bow,fired});await save();
  game.check('ordinary bow fire damages the enemy without spending mana',target(fired).hp<target(bow).hp&&fired.view.character.mp>=bow.view.character.mp);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:384},to:{x:722,y:472}}});
  game.check('bow is stored before isolated quiver casts',(await game.observe()).character.items[0]===-1);
  for(const [item,effect,cost,dex,slot] of [[0xa61,4,50,50,null],[0xade,14,65,53,0],[0xb28,14,75,55,1],[0xc07,6,100,50,2]]){
    if(slot!==null){
      await game.act({keys:[],duration_ms:350,drag:{from:{x:634+slot*44,y:428},to:{x:678,y:384}}});
      game.check(`native equip ${item.toString(16)}`,(await game.observe()).character.items[1]===item);
    }
    if(item===0xade)await game.act({keys:[],duration_ms:1000});
    if(item===0xc07){
      const empty=await game.observe();
      game.check('Thunder lacks mana after tiered casts',empty.character.mp<100&&empty.character.ability_ready_ms<=empty.game_ms);
      await game.act({keys:[' '],mouse:{x:450,y:300},duration_ms:300});
      const refused=await game.observe();
      observations.push({insufficientMana:{empty,refused}});await save();
      game.check('insufficient mana prevents casting without a debit',refused.character.mp>=empty.character.mp&&refused.character.ability_ready_ms<=refused.game_ms);
    }
    if(item===0xb28||item===0xc07){
      const depleted=await game.observe(),key=item===0xb28?'5':'8',slot=item===0xb28?8:11;
      await game.act({keys:[key],duration_ms:300});
      const restored=await game.observe();
      game.check(`ordinary potion ${key} restores mana and is consumed`,restored.character.mp>depleted.character.mp&&restored.character.items[slot]===-1);
    }
    const before=await vulnerable(),enemy=target(before);assert(enemy);
    game.check(`quiver ${item.toString(16)} equip stats`,before.view.character.stats[5]===dex&&before.view.character.stats[2]===(item===0xc07?80:75));
    if(item===0xb28)game.check('T6 defense and speed bonuses',before.view.character.stats[3]===33&&before.view.character.stats[4]===52);
    const mouse=aim(before);
    assert(mouse.x>=0&&mouse.x<600&&mouse.y>=0&&mouse.y<600,'target remains on screen');
    await game.act({keys:[' '],mouse,duration_ms:700},item===0xc07?{name:'archer-thunder',when:view=>view.game_ms>=before.view.time_ms+250&&view.character.mp<before.view.character.mp-50}:undefined);
    const after=await game.inspect(),hit=target(after);assert(hit);
    observations.push({item,before,after});await save();
    game.check(`quiver ${item.toString(16)} spends mana`,after.view.character.mp<=before.view.character.mp-cost+20);
    const status=hit.statuses[String(effect)];
    game.check(`quiver ${item.toString(16)} applies a fresh timed condition ${effect}`,status&&status[0]>=before.view.time_ms&&status[1]-status[0]===(item===0xc07?4000:3000));
    game.check(`quiver ${item.toString(16)} keeps equipped item`,after.view.character.items[1]===item);
    game.check(`quiver ${item.toString(16)} damages target`,hit.hp<enemy.hp);
    if(item===0xc07){
      // Thunder is the cooldown outlier: https://github.com/OpenRotMG/Open-RotMG-Server/blob/03d3dfbff20751a1c94bdf24990c718b9ef46614/data/objects.xml#L5697-L5727
      await game.act({keys:['4'],duration_ms:300});
      const ready=await game.observe();
      game.check('cooldown check has full mana and an unexpired deadline',ready.character.mp===100&&ready.character.items[7]===-1&&ready.character.ability_ready_ms-ready.game_ms>=1500);
      await game.act({keys:[' '],mouse,duration_ms:300});
      const denied=await game.observe();
      observations.push({cooldown:{ready,denied}});
      game.check('Thunder cooldown prevents a second mana debit',denied.character.mp>=ready.character.mp);
      game.check('Thunder cooldown deadline remains unchanged',Math.abs(denied.character.ability_ready_ms-Math.max(ready.character.ability_ready_ms,denied.game_ms))<=100);
    }
    game.check('same living Archer',after.view.lifecycle.alive&&after.view.session.character===before.view.session.character);
  }
  await game.act({keys:[],duration_ms:3500});
  const expired=await game.inspect();observations.push({expired});await save();
  game.check('same living character retains equipment',expired.view.lifecycle.alive&&expired.view.character.items[1]===0xc07&&expired.view.character.items[2]===0xa0d);
  const deadline=target(observations.find(o=>o.item===0xc07).after).statuses['6'][1];
  game.check('Thunder Daze expires after four seconds',expired.view.time_ms>=deadline&&!Object.entries(target(expired).statuses).some(([effect,[,until]])=>effect==='6'&&until>expired.view.time_ms));
}
