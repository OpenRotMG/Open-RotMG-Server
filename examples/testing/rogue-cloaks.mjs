import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

const root=process.argv[process.argv.indexOf('--run-dir')+1];
const invisible=1<<12;
export const fixture={class:0x300,level:1,seed:763,world:-2,position:'spawn',equipment:[0xa14,0xa56,0xa0d,-1],backpack:Array(8).fill(-1),stats:null};
export async function run(game){
  const observations={};
  const save=async(name,client)=>{
    const state=await client.inspect();observations[name]=state;
    await writeFile(join(root,'rogue-observations.json'),JSON.stringify(observations,null,2));return state.view;
  };
  const before=await save('lowBefore',game);
  game.check('prepared Rogue has dagger, cloak and leather armor',before.character.class===0x300&&before.character.items.slice(0,3).join()===fixture.equipment.slice(0,3).join());
  await game.act({keys:[' '],duration_ms:200});
  const cloaked=await save('lowCloaked',game);
  game.check('Shadows spends mana and grants invisibility',cloaked.character.mp<before.character.mp&&(cloaked.character.conditions&invisible)!==0);
  // Selected client shooting does not cancel stealth: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/Player.as
  await game.act({keys:['d'],mouse:{x:450,y:300},duration_ms:300});
  const shooting=await save('lowShooting',game);
  game.check('moving and firing preserve invisibility',Math.hypot(shooting.position.x-cloaked.position.x,shooting.position.y-cloaked.position.y)>0.1&&(shooting.character.conditions&invisible)!==0);
  game.check('dagger actually fired while cloaked',observations.lowShooting.diagnostic.character.lifetime.shots>observations.lowCloaked.diagnostic.character.lifetime.shots);
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
  await high.act({keys:[' '],duration_ms:200});
  const highCast=await save('highCast',high);
  game.check('Ghostly Concealment spends mana and grants invisibility',highCast.character.mp<highBefore.character.mp&&(highCast.character.conditions&invisible)!==0);
  await high.act({keys:[' '],duration_ms:200});
  const early=await save('highEarlyRepeat',high);
  game.check('early repeat does not spend another cloak cost',early.character.mp>=highCast.character.mp);
  await high.act({keys:[],duration_ms:7000});
  const ready=await save('highReady',high);
  game.check('high cloak expires and cooldown becomes ready',!(ready.character.conditions&invisible)&&ready.time_ms>=ready.character.ability_ready_ms);
  await high.act({keys:[' '],duration_ms:200});
  const again=await save('highRecast',high);
  game.check('high cloak can be used again after cooldown',again.character.mp<ready.character.mp&&(again.character.conditions&invisible)!==0);

  const plane=await game.join({...fixture,level:20,seed:765,equipment:[0xa14,0xa5a,0xa0d,-1],stats:[720,252,50,25,75,75,40,50]});
  const planeBefore=await save('planeBefore',plane);
  game.check('Planewalker equipment grants selected speed and dexterity bonuses',planeBefore.character.stats[4]===80&&planeBefore.character.stats[5]===79);
  await plane.act({keys:[' '],mouse:{x:450,y:300},duration_ms:250});
  const teleported=await save('planeTeleported',plane);
  game.check('Planewalker spends mana and grants invisibility',teleported.character.mp<planeBefore.character.mp&&(teleported.character.conditions&invisible)!==0);
  game.check('Planewalker actually teleports',Math.hypot(teleported.position.x-planeBefore.position.x,teleported.position.y-planeBefore.position.y)>1);
}
