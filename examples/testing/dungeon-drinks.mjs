import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
export const fixture={class:0x30e,level:20,seed:885,world:'beachzone',position:'spawn',equipment:[-1,-1,-1,-1],backpack:[0xb36,0xb37,0xb38,0xb39,0xb3a,0xb3b,0xb3c,0xb3d],stats:[670,385,50,25,50,50,0,0]};
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
  const restored=(before,after,key,amount)=>after.character[key]-before.character[key]>=amount&&after.character[key]-before.character[key]<=amount+2;
  for(let item=0xb36;item<=0xb3d;item++){
    await prepare(game,{kind:'vitals',hp:100,mp:100});
    const {before,after}=await use(item,item-0xb36+4),key=item%2===0?'hp':'mp',other=key==='hp'?'mp':'hp';
    game.check('wine '+item+' restores only its selected resource by 150',restored(before,after,key,150)&&after.character[other]-before.character[other]<=2);
    game.check('wine '+item+' does not cause Drunk',!(after.character.conditions&(1<<9)));
  }
  await prepare(game,{kind:'vitals',hp:100,mp:100});
  let result=await use(0x707);
  game.check('Healing Ichor restores 150 HP only',restored(result.before,result.after,'hp',150)&&result.after.character.mp-result.before.character.mp<=2);
  // Selected 123.5.1 definitions, not the September 2025 rebalance: https://www.realmeye.com/wiki/coral-juice#history
  for(const [hp,mp] of [[100,100],[670,100],[100,385],[669,384],[670,385]]){
    await prepare(game,{kind:'vitals',hp,mp});result=await use(0xc12);
    game.check('Coral Juice independently clamps both resources '+hp+'/'+mp,Math.abs(result.after.character.hp-Math.min(670,hp+100))<=2&&Math.abs(result.after.character.mp-Math.min(385,mp+100))<=2);
  }
  const origin=await game.observe();
  const near=await game.join({...fixture,world:origin.session.world,position:{x:origin.position.x+9.5,y:origin.position.y},backpack:Array(8).fill(-1)});
  const far=await game.join({...fixture,world:origin.session.world,position:{x:origin.position.x+10.5,y:origin.position.y},backpack:Array(8).fill(-1)});
  for(const item of [0xa3f,0xc1a]){
    for(const actor of [game,near,far])await prepare(actor,{kind:'vitals',hp:100,mp:100});
    const before=await Promise.all([game.observe(),near.observe(),far.observe()]);
    result=await use(item);
    const after=await Promise.all([game.observe(),near.observe(),far.observe()]),key=item===0xa3f?'hp':'mp',other=key==='hp'?'mp':'hp';
    game.check('group drink includes self and recipient at 9.5 but excludes 10.5 '+item,restored(before[0],after[0],key,100)&&restored(before[1],after[1],key,100)&&after[2].character[key]-before[2].character[key]<=2);
    game.check('group drink affects only selected resource '+item,after.every((v,i)=>v.character[other]-before[i].character[other]<=2));
    await save('group-near-'+item,near);await save('group-far-'+item,far);
  }
  const base=(await game.observe()).character.stats;
  for(const [item,boosts,duration] of [[0xc39,{max_hp:50,dexterity:10},20],[0xc3a,{max_hp:50,max_mp:50},20],[0xc3b,{attack:10,dexterity:10},20],[0xc3c,{defense:10,vitality:10},30]]){
    result=await use(item);
    game.check('cocktail applies exactly its two selected boosts '+item,Object.entries(base).every(([key,value])=>result.after.character.stats[key]===value+(boosts[key]??0))&&Boolean(result.after.character.conditions&(1<<9)));
    for(let n=0;n<8;n++)await game.act({keys:[],duration_ms:2000});
    const sober=await save('sober-'+item);
    game.check('cocktail Drunk expires before stat boosts '+item,!(sober.character.conditions&(1<<9))&&Object.entries(boosts).every(([key,amount])=>sober.character.stats[key]===base[key]+amount));
    for(let n=0;n<Math.ceil((duration-16)/2)+1;n++)await game.act({keys:[],duration_ms:2000});
    const expired=await save('expired-'+item);
    game.check('cocktail boosts expire without changing base stats '+item,JSON.stringify(expired.character.stats)===JSON.stringify(base));
  }
  const before=await game.observe();await game.act({keys:['Insert'],duration_ms:500});const after=await save('nexus');
  game.check('Nexus transfer retains living character and consumed inventory',after.world==='Nexus'&&after.lifecycle.alive&&after.session.character===before.session.character&&JSON.stringify(after.character.items)===JSON.stringify(before.character.items));
}
