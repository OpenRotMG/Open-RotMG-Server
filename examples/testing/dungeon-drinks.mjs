import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
export const fixture={class:0x30e,level:20,seed:885,world:'beachzone',position:'spawn',equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,50,25,50,50,0,0]};
export async function run(game){
  const root=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(root,'native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),samples=[];
  async function prepare(actor,change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:actor.actor,change}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function save(name,actor=game){const view=await actor.observe();samples.push({name,actor:actor.actor,view});await writeFile(join(root,'drink-observations.json'),JSON.stringify(samples,null,2));return view;}
  async function use(item,slot=4,actor=game){
    await prepare(actor,{kind:'item',slot,item});
    await actor.act({keys:[],duration_ms:200});
    const before=await save('before-'+item,actor);
    await actor.act({keys:[String(slot-3)],duration_ms:200},{name:'drink-'+item,when:v=>v.character.items[slot]===-1});
    const after=await save('after-'+item,actor);
    game.check('drink '+item+' consumes exactly one slot/revision',after.character.items[slot]===-1&&after.character.inventory_revision===before.character.inventory_revision+1&&after.character.items.every((v,i)=>i===slot||v===before.character.items[i]));
    await actor.act({keys:[],duration_ms:100});
    await actor.act({keys:[String(slot-3)],duration_ms:100});
    game.check('empty drink slot cannot repeat '+item,(await actor.observe()).character.inventory_revision===after.character.inventory_revision);
    return {before,after};
  }
  async function waitUntil(time){
    for(let n=0;n<40;n++){
      const now=(await game.observe()).game_ms;if(now>=time)return;
      await game.act({keys:[],duration_ms:Math.max(50,Math.min(2000,time-now))});
    }
    throw Error('Native game clock did not reach '+time);
  }
  const recovery=(before,after,key)=>Math.ceil((after.game_ms-before.game_ms)/1000*(key==='hp'?1+before.character.stats.vitality/8:.6+before.character.stats.wisdom/16.7))+1;
  const restored=(before,after,key,amount)=>after.character[key]-before.character[key]>=amount&&after.character[key]-before.character[key]<=amount+recovery(before,after,key);
  for(let item=0xb34;item<=0xb3d;item++){
    await prepare(game,{kind:'vitals',hp:100,mp:100});
    const {before,after}=await use(item,4+(item-0xb34)%8),key=item%2===0?'hp':'mp',other=key==='hp'?'mp':'hp',amount=item===0xb34?230:150;
    game.check('wine '+item+' restores only its selected resource by '+amount,restored(before,after,key,amount)&&after.character[other]-before.character[other]<=recovery(before,after,other));
    game.check('wine '+item+' does not cause Drunk',!(after.character.conditions&(1<<9)));
  }
  await prepare(game,{kind:'vitals',hp:100,mp:100});
  let result=await use(0x707);
  game.check('Healing Ichor restores 150 HP only',restored(result.before,result.after,'hp',150)&&result.after.character.mp-result.before.character.mp<=recovery(result.before,result.after,'mp'));
  // Keep selected restoration amounts instead of modern rebalances: https://openrotmg.org/restoration/reference-data/client-123.5.1.swf
  for(const [hp,mp] of [[100,100],[670,100],[100,385],[669,384],[670,385]]){
    await prepare(game,{kind:'vitals',hp,mp});result=await use(0xc12);
    game.check('Coral Juice independently clamps both resources '+hp+'/'+mp,['hp','mp'].every(key=>{
      const cap=result.before.character.stats[key==='hp'?'max_hp':'max_mp'],expected=Math.min(cap,result.before.character[key]+100);
      return result.after.character[key]>=expected&&result.after.character[key]<=Math.min(cap,expected+recovery(result.before,result.after,key));
    }));
  }
  const origin=await game.observe();
  const near=await game.join({...fixture,world:origin.session.world,position:{x:origin.position.x+9.5,y:origin.position.y},backpack:Array(8).fill(-1)});
  const far=await game.join({...fixture,world:origin.session.world,position:{x:origin.position.x+10.5,y:origin.position.y},backpack:Array(8).fill(-1)});
  for(const item of [0xa3f,0xc1a]){
    for(const actor of [game,near,far])await prepare(actor,{kind:'vitals',hp:100,mp:100});
    const before=[];for(const actor of [game,near,far])before.push(await save('group-before-'+item,actor));
    await use(item);
    const after=[];for(const actor of [game,near,far])after.push(await save('group-after-'+item,actor));
    const key=item===0xa3f?'hp':'mp',other=key==='hp'?'mp':'hp';
    game.check('group drink includes self and recipient at 9.5 but excludes 10.5 '+item,restored(before[0],after[0],key,100)&&restored(before[1],after[1],key,100)&&after[2].character[key]-before[2].character[key]<=recovery(before[2],after[2],key));
    game.check('group drink affects only selected resource '+item,after.every((v,i)=>v.character[other]-before[i].character[other]<=recovery(before[i],v,other)));
  }
  const base=(await game.observe()).character.stats;
  for(const [item,boosts,duration] of [[0xc39,{max_hp:50,dexterity:10},20],[0xc3a,{max_hp:50,max_mp:50},20],[0xc3b,{attack:10,dexterity:10},20],[0xc3c,{defense:10,vitality:10},30]]){
    result=await use(item);
    game.check('cocktail applies exactly its two selected boosts '+item,Object.entries(base).every(([key,value])=>result.after.character.stats[key]===value+(boosts[key]??0))&&Boolean(result.after.character.conditions&(1<<9)));
    await waitUntil(result.after.game_ms+15500);
    const sober=await save('sober-'+item);
    game.check('cocktail Drunk expires before stat boosts '+item,!(sober.character.conditions&(1<<9))&&Object.entries(boosts).every(([key,amount])=>sober.character.stats[key]===base[key]+amount));
    if(duration===30){
      await waitUntil(result.after.game_ms+23000);
      const extended=await save('extended-'+item),elapsed=extended.game_ms-result.after.game_ms;
      game.check('Lime boosts remain active beyond 20 seconds',elapsed>20000&&elapsed<30000&&Object.entries(boosts).every(([key,amount])=>extended.character.stats[key]===base[key]+amount));
    }
    await waitUntil(result.after.game_ms+duration*1000+1000);
    const expired=await save('expired-'+item);
    game.check('cocktail boosts expire without changing base stats '+item,JSON.stringify(expired.character.stats)===JSON.stringify(base));
  }
  const before=await game.observe();await game.act({keys:['Insert'],duration_ms:500});const after=await save('nexus');
  game.check('Nexus transfer retains living character and consumed inventory',after.world==='Nexus'&&after.lifecycle.alive&&after.session.character===before.session.character&&JSON.stringify(after.character.items)===JSON.stringify(before.character.items));
}
