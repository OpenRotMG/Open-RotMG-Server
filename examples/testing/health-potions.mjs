import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const root=process.env.QA_RESULTS_DIR;
const base={class:0x30e,level:20,seed:880,world:'pirate-cave',position:'spawn',equipment:[-1,-1,-1,0xacd],backpack:[0xab0,0xa22,0xaeb,0xab0,0xa22,0xaeb,0xa22,0xc3a],stats:[670,385,50,25,50,50,0,0]};
let grave;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...base,world:1,position:{x,y},guid:'potion-scout',controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root,'native/result.json'),'utf8'));
  const token=(await readFile(join(root,'native/state/control.token'),'utf8')).trim();
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  grave=map.entities.find(e=>e.kind===0x6db&&[-3,-2,-1,0].every(dx=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)].join(','))));
  assert(grave,'Expected a naturally generated Kage grave with a clear approach');
  return base;
}
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const samples=[];
  async function prepare(actor,change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor,change}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function use(client,key,slot,name){
    const before=await client.observe();
    await client.act({keys:[key],duration_ms:200},{name,when:v=>v.character.items[slot]===-1});
    const after=await client.observe();samples.push({name,before,after});
    game.check(name+' consumes exactly one slot/revision',after.character.items[slot]===-1&&after.character.inventory_revision===before.character.inventory_revision+1&&after.character.items.every((v,i)=>i===slot||v===before.character.items[i]));
    return {before,after};
  }
  await game.act({keys:[],duration_ms:500});
  game.check('equipped ring raises effective HP ceiling', (await game.observe()).character.stats.max_hp===810);
  for(const [key,slot,amount] of [['1',4,50],['2',5,100],['3',6,200]]){
    await prepare(game.actor,{kind:'vitals',hp:100,mp:100});
    const {before,after}=await use(game,key,slot,'heal-'+amount);
    game.check('native potion restores its own '+amount+' HP',after.character.hp-before.character.hp>=amount&&after.character.hp-before.character.hp<=amount+2);
    const revision=after.character.inventory_revision;
    await game.act({keys:[key],duration_ms:200});
    game.check('empty slot cannot consume twice '+amount,(await game.observe()).character.inventory_revision===revision);
  }
  for(const [key,slot,amount] of [['4',7,50],['5',8,100],['6',9,200]]){
    await prepare(game.actor,{kind:'vitals',hp:809,mp:100});
    const {after}=await use(game,key,slot,'capped-'+amount);
    game.check('native potion respects effective equipped maximum '+amount,after.character.hp===810&&after.character.stats.max_hp===810);
  }
  for(const [key,slot,item] of [['1',4,0xab0],['2',5,0xa22],['3',6,0xaeb]]){
    await prepare(game.actor,{kind:'item',slot,item});
    const {after}=await use(game,key,slot,'full-'+item);
    game.check('full HP consumes without overheal '+item,after.character.hp===810);
  }
  await use(game,'8',11,'temporary-hp-ceiling');
  game.check('ordinary Blue Paradise raises effective HP above ring ceiling',(await game.observe()).character.stats.max_hp===860);
  const {after:boosted}=await use(game,'7',10,'boosted-heal');
  game.check('potion fills temporary maximum above equipment maximum',boosted.character.hp===860);
  // Selected 123.5.1 slot attemptUse has no full-HP/Sick gate: https://openrotmg.org/restoration/reference-data/client-123.5.1.swf

  for(let n=0;n<11;n++)await game.act({keys:[],duration_ms:2000});
  game.check('temporary ceiling expires and clamps to equipped maximum',(await game.observe()).character.hp===810&&(await game.observe()).character.stats.max_hp===810);
  await prepare(game.actor,{kind:'item',slot:10,item:0xa22});
  const {after:full}=await use(game,'7',10,'full-hp');
  game.check('full HP potion is consumed without overheal',full.character.hp===810);
  await game.act({keys:['Insert'],duration_ms:500});
  const escaped=await game.observe();
  game.check('Nexus reconnect preserves consumed inventory and character',escaped.world==='Nexus'&&escaped.lifecycle.alive&&escaped.session.character===full.session.character&&escaped.character.items.slice(4).every(v=>v===-1));
  const sick=await game.join({...base,world:1,position:{x:grave.pos.x-2.5,y:grave.pos.y},equipment:[0xb08,-1,-1,-1],backpack:[0xab0,0xa22,0xaeb,-1,-1,-1,-1,-1]});
  await prepare(sick.actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});
  for(let n=0;n<25;n++){
    await sick.act({keys:[],duration_ms:200,target:{entity:grave.id,camera_angle:-Math.PI/4,fire:true}});
    if((await sick.inspect()).diagnostic.entities.find(e=>e.id===grave.id)?.stage>0)break;
  }
  for(let n=0;n<40&&!((await sick.observe()).character.conditions&16);n++)await sick.act({keys:[],duration_ms:200});
  game.check('ordinary natural Kage mine applies Sick',Boolean((await sick.observe()).character.conditions&16));
  for(const [key,slot,amount] of [['1',4,50],['2',5,100],['3',6,200]]){
    await prepare(sick.actor,{kind:'vitals',hp:100,mp:100});
    const {before,after}=await use(sick,key,slot,'sick-'+amount);
    game.check('Sick consumes but prevents healing '+amount,Boolean(before.character.conditions&16)&&Boolean(after.character.conditions&16)&&after.character.hp===100);
  }
  await writeFile(join(root,'potion-observations.json'),JSON.stringify({preparedEquipmentInventoryVitalsAndProtection:true,greaterObtainabilityNotProven:true,samples},null,2));
}
