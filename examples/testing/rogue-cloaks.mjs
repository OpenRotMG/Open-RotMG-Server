import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as wait} from 'node:timers/promises';

const root=process.argv[process.argv.indexOf('--run-dir')+1];
const invisible=1<<12;
// Paint synchronization can outlast a short cloak; observe its active window instead of the paused tail.
export async function observeAction(game,name,input,read,when){
  let done=false,seen,last;
  const action=game.act(input).finally(()=>done=true);
  const observer=(async()=>{
    while(!done){last=await read();if(!last.paused&&when(last)){seen=last;return;}await wait(50);}
  })();
  const results=await Promise.allSettled([action,observer]);
  await writeFile(join(root,name+'.json'),JSON.stringify({observed:seen!==undefined,state:last},null,2));
  for(const result of results)if(result.status==='rejected')throw result.reason;
  if(seen===undefined)throw Error(name+' was not observed during the native action');
  return seen;
}
// A one-cast MP budget prevents regeneration from obscuring the insufficient-mana check.
export const fixture={class:0x300,level:1,seed:763,world:-2,position:'spawn',equipment:[0xa14,0xa56,0xa0d,-1],backpack:Array(8).fill(-1),stats:[150,60,10,0,15,15,15,10]};
export async function run(game){
  const observations={};
  const save=async(name,client)=>{
    const state=await client.inspect();observations[name]=state;
    await writeFile(join(root,'rogue-observations.json'),JSON.stringify(observations,null,2));return state.view;
  };
  const before=await save('lowBefore',game);
  game.check('prepared Rogue has dagger, cloak and leather armor',before.character.class===0x300&&before.character.items.slice(0,3).join()===fixture.equipment.slice(0,3).join());
  // Selected client shooting does not cancel stealth: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/Player.as
  let invisibleShots;
  const shooting=await observeAction(game,'lowActive',{keys:[' ','d'],mouse:{x:450,y:300},duration_ms:1200},game.observe,v=>{
    if(!(v.character.conditions&invisible))return false;
    if(invisibleShots===undefined)invisibleShots=v.verification.shots;
    return v.verification.shots>invisibleShots&&Math.hypot(v.position.x-before.position.x,v.position.y-before.position.y)>0.1;
  });
  game.check('Shadows spends mana and grants invisibility',shooting.character.mp<before.character.mp&&(shooting.character.conditions&invisible)!==0);
  game.check('moving and another admitted dagger shot preserve invisibility',shooting.verification.shots>invisibleShots&&Math.hypot(shooting.position.x-before.position.x,shooting.position.y-before.position.y)>0.1);
  await save('lowAfterActive',game);
  await game.act({keys:[],duration_ms:4200});
  const expired=await save('lowExpired',game);
  game.check('Shadows expires after its duration',!(expired.character.conditions&invisible));
  game.check('low Rogue lacks mana for a second cloak',expired.character.mp<60);
  await game.act({keys:[' '],duration_ms:200});
  const denied=await save('lowManaDenied',game);
  game.check('insufficient-mana input leaves Rogue visible without spending mana',!(denied.character.conditions&invisible)&&denied.character.mp>=expired.character.mp);

  const high=await game.join({...fixture,level:20,seed:764,equipment:[0xa14,0xb27,0xa0d,-1],stats:[720,252,50,25,75,75,40,50]});
  const highBefore=await save('highBefore',high);
  game.check('Ghostly Concealment equipment bonuses match selected definitions',highBefore.character.stats[4]===80&&highBefore.character.stats[5]===79&&highBefore.character.stats[6]===42&&highBefore.character.stats[7]===52);
  const highCast=await observeAction(high,'highCast',{keys:[' '],duration_ms:200},high.observe,v=>!!(v.character.conditions&invisible)&&v.character.mp<highBefore.character.mp);
  game.check('Ghostly Concealment spends mana and grants invisibility',highCast.character.mp<highBefore.character.mp&&(highCast.character.conditions&invisible)!==0);
  game.check('high cloak starts its cooldown',highCast.character.ability_ready_ms>highCast.game_ms);
  await high.act({keys:[],duration_ms:7000});
  const ready=await save('highReady',high);
  game.check('high cloak expires and cooldown becomes ready',!(ready.character.conditions&invisible)&&ready.time_ms>=ready.character.ability_ready_ms);
  const again=await observeAction(high,'highRecast',{keys:[' '],duration_ms:200},high.observe,v=>!!(v.character.conditions&invisible)&&v.character.mp<ready.character.mp);
  game.check('high cloak can be used again after cooldown',again.character.mp<ready.character.mp&&(again.character.conditions&invisible)!==0);

  const plane=await game.join({...fixture,level:20,seed:765,equipment:[0xa14,0xa5a,0xa0d,-1],stats:[720,252,50,25,75,75,40,50]});
  const planeBefore=await save('planeBefore',plane);
  game.check('Planewalker equipment grants selected speed and dexterity bonuses',planeBefore.character.stats[4]===80&&planeBefore.character.stats[5]===79);
  const teleported=await observeAction(plane,'planeTeleported',{keys:[' '],mouse:{x:450,y:300},duration_ms:250},plane.observe,v=>!!(v.character.conditions&invisible)&&Math.hypot(v.position.x-planeBefore.position.x,v.position.y-planeBefore.position.y)>1);
  game.check('Planewalker spends mana and grants invisibility',teleported.character.mp<planeBefore.character.mp&&(teleported.character.conditions&invisible)!==0);
  game.check('Planewalker actually teleports',Math.hypot(teleported.position.x-planeBefore.position.x,teleported.position.y-planeBefore.position.y)>1);
}
