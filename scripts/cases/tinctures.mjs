import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const root=process.env.QA_RESULTS_DIR;
export const fixture={class:0x30e,level:20,seed:884,world:-4,position:'spawn',equipment:[-1,-1,-1,-1],backpack:[0xb0c,0xb0c,0xb0d,0xb0e,0xb0f,-1,-1,-1],stats:[670,385,50,25,50,60,0,0]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),samples=[];
  async function prepare(actor,change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor,change}})});
    const result=await response.json();assert(response.ok&&result.status==='succeeded',JSON.stringify(result));
  }
  async function use(client,key,slot,name){
    const before=await client.observe();
    await client.act({keys:[key],duration_ms:200},{name,when:v=>v.character.items[slot]===-1});
    const after=await client.observe();samples.push({name,before,after});
    game.check(name+' consumes exactly once',after.character.items[slot]===-1&&after.character.inventory_revision===before.character.inventory_revision+1&&after.character.items.every((v,i)=>i===slot||v===before.character.items[i]));
    return after;
  }
  async function wait(ms){while(ms>0){const step=Math.min(ms,2000);await game.act({keys:[],duration_ms:step});ms-=step;}}
  await game.act({keys:[],duration_ms:200});
  const baseline=await game.observe();
  await use(game,'1',4,'first-dexterity');
  game.check('first tincture adds 15 DEX',(await game.observe()).character.stats.dexterity===75);
  await wait(5000);
  const doubled=await use(game,'2',5,'overlapping-dexterity');
  await game.act({keys:[],duration_ms:600});
  await game.act({keys:[],duration_ms:200},{name:'stacked-dexterity',when:()=>true});
  await writeFile(join(root,'stacking-observation.json'),JSON.stringify({baseline,doubled},null,2));
  game.check('second tincture contributes floor(15/2), not another 15',doubled.character.stats.dexterity===82);
  const firstUntil=baseline.game_ms+31300;
  await wait(firstUntil-(await game.observe()).game_ms+100);
  game.check('first expiry does not refresh the first timer; remaining dose has full strength',(await game.observe()).character.stats.dexterity===75);
  await wait(5500);
  game.check('both independent doses expire',(await game.observe()).character.stats.dexterity===60);
  await prepare(game.actor,{kind:'vitals',hp:100,mp:100});
  for(const [key,slot,stat,amount] of [['3',6,'max_hp',75],['4',7,'max_mp',75],['5',8,'defense',15]]){
    const before=await game.observe(),after=await use(game,key,slot,'self-'+stat);
    game.check('self tincture raises '+stat,after.character.stats[stat]===before.character.stats[stat]+amount);
    if(stat==='max_hp')game.check('maximum HP increase is not an instant heal',after.character.hp-before.character.hp<5);
    if(stat==='max_mp')game.check('maximum MP increase is not an instant refill',after.character.mp-before.character.mp<5);
  }
  await wait(31000);
  const expired=await game.observe();
  game.check('all self stats expire without altering base stats',JSON.stringify(expired.character.stats)===JSON.stringify(baseline.character.stats));
  const pos=expired.position;
  const inside=await game.join({...fixture,position:{x:pos.x+5.75,y:pos.y},backpack:Array(8).fill(-1)});
  const boundary=await game.join({...fixture,position:{x:pos.x+6,y:pos.y},backpack:Array(8).fill(-1)});
  for(let i=0;i<4;i++)await prepare(game.actor,{kind:'item',slot:4+i,item:0xb10+i});
  await game.act({keys:[],duration_ms:200});
  for(const [i,stat,amount] of [[0,'dexterity',30],[1,'max_hp',150],[2,'max_mp',150],[3,'defense',30]]){
    const before=await game.observe(),near=await inside.observe(),edge=await boundary.observe();
    const after=await use(game,String(i+1),i+4,'aura-'+stat);
    const received=await inside.observe(),excluded=await boundary.observe();
    samples.push({name:'recipients-'+stat,near,received,edge,excluded});
    game.check('effusion includes user '+stat,after.character.stats[stat]===before.character.stats[stat]+amount);
    game.check('effusion includes nearby unrelated ally '+stat,received.character.stats[stat]===near.character.stats[stat]+amount);
    game.check('effusion excludes exact six-tile boundary '+stat,excluded.character.stats[stat]===edge.character.stats[stat]);
  }
  await prepare(game.actor,{kind:'vitals',hp:670,mp:385});
  for(const [slot,item] of [[8,0xaeb],[9,0xaec]])await prepare(game.actor,{kind:'item',slot,item});
  await game.act({keys:[],duration_ms:200});
  for(const [key,slot] of [['5',8],['6',9]])await use(game,key,slot,'refill-'+slot);
  const filled=await game.observe();
  game.check('ordinary healing and mana potions fill temporary maxima',filled.character.hp===820&&filled.character.mp===535);
  await wait(31000);
  const clamped=await game.observe();
  game.check('expiry clamps HP and restores every aura stat',clamped.character.hp===670&&clamped.character.mp===385&&JSON.stringify(clamped.character.stats)===JSON.stringify(baseline.character.stats));
  game.check('nearby ally aura expires',JSON.stringify((await inside.observe()).character.stats)===JSON.stringify(baseline.character.stats));
  await prepare(game.actor,{kind:'item',slot:4,item:0xb0d});
  await game.act({keys:[],duration_ms:200});await use(game,'1',4,'reconnect-life');
  const boosted=await game.observe();game.check('fresh life tincture active before reconnect',boosted.character.stats.max_hp===745);
  await game.act({keys:['Insert'],duration_ms:500});
  const reconnected=await game.observe();
  game.check('normal reconnect preserves identity and consumption but not temporary boosts',reconnected.session.character===boosted.session.character&&reconnected.character.items[4]===-1&&reconnected.character.stats.max_hp===670);
  const revision=reconnected.character.inventory_revision;
  await game.act({keys:['1'],duration_ms:200});
  game.check('empty slot cannot publish another effect',(await game.observe()).character.inventory_revision===revision&&(await game.observe()).character.stats.max_hp===670);
  await writeFile(join(root,'tincture-observations.json'),JSON.stringify({preparedInventoryStatsVitalsAndPositions:true,earnedAcquisitionNotProven:true,samples},null,2));
}
