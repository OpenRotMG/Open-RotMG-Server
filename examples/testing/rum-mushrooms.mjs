import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
export const fixture={class:0x30e,level:20,seed:886,world:'pirate-cave',position:'spawn',equipment:[-1,-1,-1,-1],backpack:[0xa50,0xa51,0xc25,0xc25,0xa50,0xa51,0xc25,0xa47],stats:null};
export async function run(game){
  const samples=[],drunk=1<<9,hallucinating=1<<8,invisible=1<<12;
  const initial=await game.observe();samples.push(initial);
  const step=(keys,duration_ms)=>({actor:game.actor,input:{keys,duration_ms}});
  async function act(input,capture){
    let done=false;
    const action=game.act(input,capture).finally(()=>done=true);
    const poll=(async()=>{while(!done){samples.push(await game.observe());await delay(100);}})();
    const results=await Promise.allSettled([action,poll]);
    await writeFile(join(process.env.QA_RESULTS_DIR,'rum-observations.json'),JSON.stringify({preparedGearAndLevel:true,earnedDropsAndFullAiNotTested:true,samples},null,2));
    for(const result of results)if(result.status==='rejected')throw result.reason;
    samples.push(await game.observe());
  }
  async function until(time){while((await game.observe()).game_ms<time)await act({keys:[],duration_ms:Math.min(8000,time-(await game.observe()).game_ms)});}
  const useIndex=slot=>samples.findIndex(v=>v.character.items[slot]===-1);
  const used=slot=>samples[useIndex(slot)];
  await act({keys:[],duration_ms:2000},{name:'sober',when:v=>v.game_ms>initial.game_ms+500});
  await act({keys:['2'],duration_ms:2000},{name:'mushroom',when:v=>Boolean(v.character.conditions&hallucinating)});
  game.check('Mushroom alone applies Hallucinating',samples.some(v=>v.character.conditions===hallucinating));
  await act([step(['1'],100),step([],1000),step(['3'],100),step([],700),step(['4'],100),step([],6000)],{name:'rum-and-ghost',when:v=>v.character.conditions===(drunk|hallucinating|invisible)});
  game.check('Rum and Ghost Rum coexist with Mushroom',samples.some(v=>v.character.conditions===(drunk|hallucinating|invisible)));
  game.check('second Ghost Rum is used while the first invisibility is active',useIndex(7)>0&&Boolean(samples[useIndex(7)-1].character.conditions&invisible));
  const ghostWindow=samples.filter(v=>v.game_ms>used(6).game_ms+4200&&v.game_ms<used(7).game_ms+3500);
  game.check('refreshed invisibility outlasts first Ghost Rum deadline',ghostWindow.length>0&&ghostWindow.every(v=>v.character.conditions&invisible));
  await until(used(4).game_ms+18000);
  game.check('short Ghost Rum does not truncate the longer Drunk',Boolean((await game.observe()).character.conditions&drunk));
  await act([step(['5'],100),step(['6'],100),step([],1000)]);
  for(const [slot,mask] of [[8,drunk],[9,hallucinating]])game.check('30-second condition refresh occurs while active in slot '+slot,Boolean(samples[useIndex(slot)-1].character.conditions&mask));
  await until(Math.max(used(8).game_ms,used(9).game_ms)+32000);
  for(const [first,second,mask,name] of [[4,8,drunk,'Drunk'],[5,9,hallucinating,'Hallucinating']]){
    const refreshedWindow=samples.filter(v=>v.game_ms>used(first).game_ms+31000&&v.game_ms<used(second).game_ms+28000);
    game.check(name+' survives its original deadline after refresh',refreshedWindow.length>0&&refreshedWindow.every(v=>v.character.conditions&mask));
    bracket(second,mask,30000,name+' refreshed expiration');
  }
  game.check('both long effects expire without another item',(await game.observe()).character.conditions===0);
  // Keep overlap qualified: Kithio replaces timers; our engine retains later deadlines (https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/Entity.cs#L377-L394).
  await act([step(['7'],100),step([],9000)],{name:'ghost-alone',when:v=>v.character.conditions===(drunk|invisible)});
  bracket(10,invisible,4000,'Ghost Invisible expiration');bracket(10,drunk,6000,'Ghost Drunk expiration');
  game.check('Ghost Invisible expires before its Drunk',samples.some(v=>v.game_ms>used(10).game_ms&&v.character.conditions===drunk));
  const consumed=await game.observe();
  await act([step(['7'],100),step(['8'],100)]);
  const refused=await game.observe();
  game.check('seven ordinary uses consume exactly seven revisions and retain the incompatible sword',consumed.character.inventory_revision===initial.character.inventory_revision+7&&consumed.character.items.slice(4,11).every(v=>v===-1)&&consumed.character.items[11]===0xa47);
  game.check('empty reuse and incompatible sword are atomic refusals',refused.character.inventory_revision===consumed.character.inventory_revision&&JSON.stringify(refused.character.items)===JSON.stringify(consumed.character.items)&&refused.character.conditions===0);
  game.check('consumables never alter HP, MP or stats',samples.every(v=>v.character.hp===initial.character.hp&&v.character.mp===initial.character.mp&&JSON.stringify(v.character.stats)===JSON.stringify(initial.character.stats)));
  await game.act({keys:['Insert'],duration_ms:1000});
  const reconnected=await game.observe();
  game.check('actual cave-to-Nexus reconnect preserves living character and consumed inventory',reconnected.world==='Nexus'&&reconnected.session.world!==initial.session.world&&reconnected.lifecycle.alive&&reconnected.session.character===initial.session.character&&reconnected.character.inventory_revision===refused.character.inventory_revision&&JSON.stringify(reconnected.character.items)===JSON.stringify(refused.character.items));
  samples.push(reconnected);
  await writeFile(join(process.env.QA_RESULTS_DIR,'rum-observations.json'),JSON.stringify({preparedGearAndLevel:true,earnedDropsAndFullAiNotTested:true,samples},null,2));
  function bracket(slot,mask,duration,name){
    const index=useIndex(slot),before=samples[index-1],applied=samples[index];
    const cleared=samples.findIndex((v,i)=>i>index&&!(v.character.conditions&mask)),last=samples[cleared-1],after=samples[cleared];
    game.check(name+' brackets selected duration on observed clock',index>0&&cleared>index&&last.game_ms-applied.game_ms<=duration&&after.game_ms-before.game_ms>=duration);
    const window=samples.slice(index-1,cleared+1);
    game.check(name+' has no sampling gap above 500ms',window.every((v,i)=>i===0||v.game_ms-window[i-1].game_ms<=500));
  }
}
