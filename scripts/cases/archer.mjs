import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {inRange} from './septavius-range.mjs';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x307,level:20,seed:764,world:'undead-lair',worldSeed:12351,position:{x:13.5,y:120.5},equipment:[-1,0xa61,-1,-1],backpack:[-1,0xb28,0xc07,0xa23,-1,0xa0d,0xa1a,0xa23],stats:[700,100,75,25,50,50,40,50]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const initial=await game.inspect();
  const boss=initial.view.objects.find(e=>e.kind===0xd90);assert(boss,'prepared Archer sees Septavius');
  const protection=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'archer-incoming-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}),signal:AbortSignal.timeout(10000)});
  const protectedState=await protection.json();assert(protection.ok&&protectedState.status!=='failed',JSON.stringify(protectedState));
  const observations=[{incomingProtection:protectedState}];
  const save=()=>writeFile(join(root,'archer-observations.json'),JSON.stringify(observations,null,2));
  const target=state=>state.diagnostic.entities.find(e=>e.kind===0xd90);
  const shot={entity:boss.id,camera_angle:-Math.PI/4,fire:false};
  // XML Septavius is Invulnerable in most Triforce/Clear_the_room substates (actors.xml:9725 Septavius); wait for an observed opening.
  const vulnerable=async cost=>{
    for(let attempt=0;attempt<80;attempt++){
      const state=await game.inspect(),enemy=target(state),c=state.view.character;assert(enemy);
      if(!(enemy.condition&(1<<24))&&c.mp>=cost&&c.ability_ready_ms<=state.view.time_ms)return state;
      await act({keys:[],duration_ms:100});
    }
    throw Error('No bounded affordable Septavius vulnerability window observed');
  };
  // A cast lands ~1s after its input and Invulnerable still takes the condition (native_hits.rs), so short openings show the
  // status without damage; Start_over_again is Septavius's only multi-second vulnerable state (4s after Summon_the_undead).
  const phase=state=>state.diagnostic.xml[target(state).id]?.states[1]?.name;
  const opening=async cost=>{
    for(let attempt=0;attempt<85;attempt++){
      const state=await inRange({inspect:game.inspect,act},boss,5),c=state.view.character;
      if(phase(state)==='Start_over_again'&&!(c.conditions&2)&&c.ability_ready_ms<=state.view.time_ms){
        if(c.mp<cost)await vitals(c.hp,cost);
        return game.inspect();
      }
      await act({keys:[],duration_ms:phase(state)==='Summon_the_undead'?900:2300});
    }
    throw Error('No bounded affordable Septavius Start_over_again opening observed');
  };
  let serial=0,shielded=initial.view.time_ms+60000;
  const cheat=async change=>{
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'archer-cheat-'+serial++,command:{op:'cheat',actor:game.actor,change}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  };
  const vitals=async(hp,mp)=>{await cheat({kind:'vitals',hp,mp});observations.push({preparedVitals:{hp,mp}});};
  // Protection cannot be renewed while active, and loaded acts can overrun: idle out the window, then renew.
  async function act(input,capture){
    let now=(await game.observe()).game_ms;
    while(now<shielded&&shielded-now<input.duration_ms+5000){await game.act({keys:[],duration_ms:Math.min(10000,Math.max(50,shielded-now))});now=(await game.observe()).game_ms;}
    if(now>=shielded){await cheat({kind:'protection',condition:'invulnerable',duration_ms:60000});shielded=now+60000;}
    return game.act(input,capture);
  }
  let armored;
  for(let n=0;n<4;n++){
    await act({keys:[],duration_ms:500,drag:{from:{x:678,y:472},to:{x:722,y:384}}});
    armored=await game.observe();if(armored.character.items[2]===0xa0d)break;
  }
  observations.push({armored});await save();
  game.check('native leather armor equips and grants defense',armored.character.items[2]===0xa0d&&armored.character.stats.defense===31);
  await act({keys:[],duration_ms:350,drag:{from:{x:722,y:472},to:{x:634,y:384}}});
  const bow=await vulnerable(0);
  game.check('native Archer equips a bow',bow.view.character.items[0]===0xa1a);
  // Triforce opens Septavius for 0.6s of every 1.6s (actors.xml:9725); span a full cycle so a hit lands in an observed opening.
  await act({keys:[],target:{...shot,fire:true},duration_ms:2000});
  const fired=await game.inspect();observations.push({bow,fired});await save();
  game.check('ordinary bow fire damages the enemy without spending mana',target(fired).hp<target(bow).hp&&fired.view.character.mp>=bow.view.character.mp);
  await act({keys:[],duration_ms:350,drag:{from:{x:634,y:384},to:{x:722,y:472}}});
  game.check('bow is stored before isolated quiver casts',(await game.observe()).character.items[0]===-1);
  for(const [item,effect,cost,dex,slot] of [[0xa61,4,50,50,null],[0xb28,14,75,55,1],[0xc07,6,100,50,2]]){
    if(slot!==null){
      await act({keys:[],duration_ms:350,drag:{from:{x:634+slot*44,y:428},to:{x:678,y:384}}});
      game.check(`native equip ${item.toString(16)}`,(await game.observe()).character.items[1]===item);
    }
    if(item===0xc07){
      const empty=await game.observe();
      game.check('Thunder lacks mana after tiered casts',empty.character.mp<100&&empty.character.ability_ready_ms<=empty.game_ms);
      await act({keys:[' '],target:shot,duration_ms:300});
      const refused=await game.observe();
      observations.push({insufficientMana:{empty,refused}});await save();
      game.check('insufficient mana prevents casting without a debit',refused.character.mp>=empty.character.mp&&refused.character.ability_ready_ms<=refused.game_ms);
      await act({keys:['8'],duration_ms:500});
      const restored=await game.observe();
      game.check('ordinary potion restores mana and is consumed',restored.character.mp>refused.character.mp&&restored.character.items[11]===-1);
    }
    let before,after,affected,hit,cast;
    // A cast can still land as the boss re-enters an Invulnerable substate: retry from the same mana.
    for(let attempt=0;attempt<6;attempt++){
      if(attempt)await vitals(before.view.character.hp,before.view.character.mp);
      before=await opening(cost);
      await act({keys:[' '],target:shot,duration_ms:700},item===0xc07&&!attempt?{name:'archer-thunder',when:view=>view.game_ms>=before.view.time_ms+250&&view.character.mp<before.view.character.mp-50}:undefined);
      // Client hit reports can trail the cast under load, so give the damage a few observed ticks to land.
      for(let n=0;n<6;n++){
        after=await game.inspect();if(!n)cast=after;
        affected=after.diagnostic.entities.filter(e=>{const status=e.statuses[String(effect)];return status&&status[0]>=before.view.time_ms;});
        hit=item===0xc07?affected[0]:target(after);
        if(hit&&hit.hp<before.diagnostic.entities.find(e=>e.id===hit.id).hp)break;
        await act({keys:[],duration_ms:300});
      }
      observations.push({item,attempt,before,after,affected:affected.map(e=>e.id)});await save();
      if(hit&&hit.hp<before.diagnostic.entities.find(e=>e.id===hit.id).hp)break;
    }
    game.check(`quiver ${item.toString(16)} equip stats`,before.view.character.stats[5]===dex&&before.view.character.stats[2]===(item===0xc07?80:75));
    if(item===0xb28)game.check('T6 Vitality and Wisdom bonuses',before.view.character.stats[6]===42&&before.view.character.stats[7]===52);
    assert(hit,'quiver condition reaches a living enemy');
    if(item===0xc07)game.check('Thunder affects exactly one enemy',affected.length===1);
    game.check(`quiver ${item.toString(16)} spends mana`,cast.view.character.mp<=before.view.character.mp-cost+20);
    const status=hit.statuses[String(effect)];
    game.check(`quiver ${item.toString(16)} applies a fresh timed condition ${effect}`,status&&status[0]>=before.view.time_ms&&status[1]-status[0]===(item===0xc07?4000:3000));
    game.check(`quiver ${item.toString(16)} keeps equipped item`,after.view.character.items[1]===item);
    game.check(`quiver ${item.toString(16)} damages target`,hit.hp<before.diagnostic.entities.find(e=>e.id===hit.id).hp);
    if(item===0xc07){
      // Thunder is the cooldown outlier: https://github.com/OpenRotMG/Open-RotMG-Server/blob/03d3dfbff20751a1c94bdf24990c718b9ef46614/data/objects.xml#L5697-L5727
      const retry=await act([
        {actor:game.actor,input:{keys:['4'],duration_ms:300}},
        {actor:game.actor,input:{keys:[' '],target:shot,duration_ms:300}},
      ]);
      const ready=retry.steps[0],denied=retry.state;
      observations.push({cooldown:{ready,denied}});
      game.check('cooldown check had full mana and an unexpired deadline',ready.character.mp===100&&ready.character.items[7]===-1&&ready.character.ability_ready_ms-ready.game_ms>=1500);
      game.check('Thunder cooldown prevents a second mana debit',denied.character.mp===ready.character.mp);
      game.check('Thunder cooldown deadline remains unchanged',Math.abs(denied.character.ability_ready_ms-Math.max(ready.character.ability_ready_ms,denied.game_ms))<=100);
    }
    game.check('same living Archer',after.view.lifecycle.alive&&after.view.session.character===before.view.session.character);
  }
  const thunder=observations.findLast(o=>o.item===0xc07),id=thunder.affected[0];
  const deadline=thunder.after.diagnostic.entities.find(e=>e.id===id).statuses['6'][1];
  const now=(await game.observe()).game_ms;
  if(now<deadline)await act({keys:[],duration_ms:deadline-now+50});
  const expired=await game.inspect();observations.push({expired});await save();
  game.check('same living character retains equipment',expired.view.lifecycle.alive&&expired.view.character.items[1]===0xc07&&expired.view.character.items[2]===0xa0d);
  const survivor=expired.diagnostic.entities.find(e=>e.id===id);assert(survivor,'Dazed enemy remains observed for expiry');
  game.check('Thunder Daze expires after four seconds',expired.view.time_ms>=deadline&&!Object.entries(survivor.statuses).some(([effect,[,until]])=>effect==='6'&&until>expired.view.time_ms));
}
