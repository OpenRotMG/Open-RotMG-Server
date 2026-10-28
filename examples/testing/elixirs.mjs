import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const root=process.env.QA_RESULTS_DIR;
const base={class:0x30e,level:20,seed:883,world:'pirate-cave',position:'spawn',equipment:[-1,-1,-1,-1],backpack:[0xb14,0xb18,0xb14,0xb18,-1,-1,-1,-1],stats:[670,385,50,25,50,50,0,0]};
const health=[0xb14,0xb15,0xb16,0xb17,0xa81,0xa48,0xa49,-1],magic=[0xb18,0xb19,0xb1a,0xb1b,0xa4a,0xaed,0xaee,-1];
let grave;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...base,world:1,position:{x,y},guid:'elixir-scout',controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root,'native/result.json'),'utf8'));
  const token=(await readFile(join(root,'native/state/control.token'),'utf8')).trim();
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  grave=map.entities.find(e=>e.kind===0x6db&&[-3,-2,-1,0].every(dx=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)].join(','))));
  assert(grave,'Expected a natural Kage grave with a clear approach');
  return base;
}
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),samples=[];
  async function prepare(actor,change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor,change}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function use(client,key,slot,next,name,capture=false){
    const before=await client.observe();
    await client.act({keys:[key],duration_ms:250},capture?{name,when:v=>v.character.items[slot]===next}:undefined);
    const after=await client.observe();samples.push({name,before,after});
    game.check(name+' replaces only its slot exactly once',after.character.items[slot]===next&&after.character.inventory_revision===before.character.inventory_revision+1&&after.character.items.every((v,i)=>i===slot||v===before.character.items[i]));
    game.check(name+' preserves stats and living identity',JSON.stringify(after.character.stats)===JSON.stringify(before.character.stats)&&after.lifecycle.alive&&after.session.character===before.session.character);
    return {before,after};
  }
  for(const [key,slot,chain,resource] of [['1',4,health,'hp'],['2',5,magic,'mp']]){
    for(let i=0;i<7;i++){
      await prepare(game.actor,{kind:'vitals',hp:100,mp:100});
      const {before,after}=await use(game,key,slot,chain[i+1],resource+'-dose-'+(7-i),i===5);
      game.check(resource+' dose '+(7-i)+' restores 125',after.character[resource]-before.character[resource]>=125&&after.character[resource]-before.character[resource]<=126);
      const other=resource==='hp'?'mp':'hp';
      game.check(resource+' dose '+(7-i)+' does not restore '+other,after.character[other]-before.character[other]<=1);
    }
    const before=await game.observe();await game.act({keys:[key],duration_ms:250});
    game.check(resource+' empty final slot cannot consume twice',(await game.observe()).character.inventory_revision===before.character.inventory_revision);
  }
  // Keep ineffective-use consumption: selected 123.5.1 attemptUse has no HP/MP/Sick gate (https://openrotmg.org/restoration/reference-data/client-123.5.1.swf).
  for(const [hp,mp,label,index] of [[670,385,'full',1],[669,384,'capped',2]]){
    await prepare(game.actor,{kind:'vitals',hp,mp});
    await use(game,'3',6,health[index],label+'-health');
    await use(game,'4',7,magic[index],label+'-magic',true);
    const view=await game.observe();game.check(label+' use respects both maxima',view.character.hp===670&&view.character.mp===385);
  }
  const beforeTravel=await game.observe();
  await game.act({keys:['Insert'],duration_ms:500});
  const escaped=await game.observe();
  game.check('Nexus reconnect preserves partial doses and identity',escaped.world==='Nexus'&&escaped.session.character===beforeTravel.session.character&&JSON.stringify(escaped.character.items)===JSON.stringify(beforeTravel.character.items));
  const sick=await game.join({...base,world:1,position:{x:grave.pos.x-2.5,y:grave.pos.y},equipment:[0xb08,-1,-1,-1],backpack:[health[0],magic[0],-1,-1,-1,-1,-1,-1]});
  await prepare(sick.actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});
  for(let n=0;n<25;n++){
    await sick.act({keys:[],duration_ms:200,target:{entity:grave.id,camera_angle:-Math.PI/4,fire:true}});
    if((await sick.inspect()).diagnostic.entities.find(e=>e.id===grave.id)?.stage>0)break;
  }
  for(let i=0;i<7;i++){
    for(let n=0;n<40&&!((await sick.observe()).character.conditions&16);n++)await sick.act({keys:[],duration_ms:200});
    await prepare(sick.actor,{kind:'vitals',hp:100,mp:100});
    const h=await use(sick,'1',4,health[i+1],'sick-health-'+(7-i),i===6);
    game.check('Sick blocks health dose '+(7-i)+' but not consumption',Boolean(h.before.character.conditions&16)&&Boolean(h.after.character.conditions&16)&&h.after.character.hp===100);
    const m=await use(sick,'2',5,magic[i+1],'sick-magic-'+(7-i),i===6);
    game.check('Sick permits magic dose '+(7-i),Boolean(m.before.character.conditions&16)&&Boolean(m.after.character.conditions&16)&&m.after.character.hp===100&&m.after.character.mp-m.before.character.mp>=125&&m.after.character.mp-m.before.character.mp<=126);
  }
  await writeFile(join(root,'elixir-observations.json'),JSON.stringify({preparedStatsInventoryVitalsAndProtection:true,samples},null,2));
}
