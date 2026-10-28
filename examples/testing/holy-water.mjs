import assert from 'node:assert/strict';
import {readFile,appendFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const root=process.env.QA_RESULTS_DIR;
const base={class:0x31d,level:20,seed:887,world:'pirate-cave',position:'spawn',equipment:[-1,0xb29,-1,-1],backpack:[0xc21,0xc21,0xc21,0xc21,0xa50,0xa51,0xc41,0xc21],stats:[670,252,50,25,50,50,0,0]};
let grave,realmSpawn;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  realmSpawn={x,y};
  const scout=await api({op:'spawn',spec:{...base,world:1,position:{x,y},guid:'holy-water-scout',controller:'direct',profile:null}});
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
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const bit=index=>1<<(index-1),negative=bit(9)|bit(10),speedy=bit(15),berserk=bit(20),hexed=bit(28);
  async function prepare(actor,change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor,change}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  const regen=(sample,rate)=>Math.ceil(rate*(sample.after.game_ms-sample.before.game_ms)/1000)+1;
  const hpRegen=sample=>regen(sample,1+sample.before.character.stats.vitality/8);
  async function use(client,key,slot,name,capture=false){
    const before=await client.observe();
    await client.act({keys:[key],duration_ms:capture?1000:200},capture?{name,when:v=>v.character.items[slot]===-1}:undefined);
    const after=await client.observe();
    await appendFile(join(root,'holy-water-observations.jsonl'),JSON.stringify({name,before,after})+'\n');
    game.check(name+' keeps the same living character',after.lifecycle.alive&&after.session.character===before.session.character);
    game.check(name+' consumes one item/revision only',after.character.items[slot]===-1&&after.character.inventory_revision===before.character.inventory_revision+1&&after.character.items.every((v,i)=>i===slot||v===before.character.items[i]));
    game.check(name+' spends no MP and preserves stats',after.character.mp>=before.character.mp&&after.character.mp<=before.character.mp+regen({before,after},0.6+before.character.stats.wisdom/16.7)&&JSON.stringify(after.character.stats)===JSON.stringify(before.character.stats));
    return {before,after};
  }
  await game.act({keys:[],duration_ms:500});
  await prepare(game.actor,{kind:'vitals',hp:100,mp:252});
  let sample=await use(game,'1',4,'holy-water-heal',true);
  game.check('Holy Water restores 100 HP plus bounded regeneration',sample.after.character.hp-sample.before.character.hp>=100&&sample.after.character.hp-sample.before.character.hp<=100+hpRegen(sample));
  await prepare(game.actor,{kind:'vitals',hp:669,mp:252});
  sample=await use(game,'2',5,'holy-water-capped');
  game.check('healing caps at effective maximum HP',sample.after.character.hp===670);
  sample=await use(game,'3',6,'holy-water-full');
  game.check('full HP use still consumes without overheal',sample.after.character.hp===670);
  const revision=sample.after.character.inventory_revision;
  await game.act({keys:['3'],duration_ms:200});
  game.check('empty-slot retry has no inventory effect',(await game.observe()).character.inventory_revision===revision);
  await use(game,'5',8,'pirate-rum');
  await use(game,'6',9,'magic-mushroom');
  game.check('ordinary items apply Drunk and Hallucinating',((await game.observe()).character.conditions&negative)===negative);
  sample=await use(game,'4',7,'holy-water-cleanse');
  game.check('Holy Water removes Drunk and Hallucinating',!(sample.after.character.conditions&negative));
  await prepare(game.actor,{kind:'item',slot:4,item:0xc21});
  await game.act({keys:[],duration_ms:200});
  await game.act({keys:[' '],duration_ms:150});
  game.check('Warrior helmet supplies Speedy and Berserk',((await game.observe()).character.conditions&(speedy|berserk))===(speedy|berserk));
  sample=await use(game,'8',11,'holy-water-unhexed');
  game.check('unhexed Speedy and Berserk survive Holy Water',(sample.after.character.conditions&(speedy|berserk))===(speedy|berserk));
  await use(game,'7',10,'transformation-potion');
  game.check('Transformation Potion applies Hexed',(await game.observe()).character.conditions&hexed);
  const transformed=await game.observe(),cooldown=transformed.character.ability_ready_ms-transformed.game_ms;
  if(cooldown>0)await game.act({keys:[],duration_ms:cooldown});
  await game.act({keys:[' '],duration_ms:1000},{name:'holy-water-hexed-before',when:v=>(v.character.conditions&(hexed|speedy|berserk))===(hexed|speedy|berserk)});
  // Selected 123.5.1 0xc21 orders Heal, Hexed-gated Speedy clear, then cleanse; Kithio's NegativeEffs: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/entities/player/Player.UseItem.cs#L14-L86
  sample=await use(game,'1',4,'holy-water-hexed-after',true);
  game.check('Hexed and associated Speedy clear while Berserk survives',!(sample.after.character.conditions&(hexed|speedy))&&Boolean(sample.after.character.conditions&berserk));
  await game.act({keys:[],duration_ms:700},{name:'holy-water-restored',when:v=>!(v.character.conditions&hexed)});
  const sick=await game.join({...base,class:0x30e,world:1,position:{x:grave.pos.x-2.5,y:grave.pos.y},equipment:[0xb08,-1,-1,-1],backpack:[0xc21,0xc21,-1,-1,-1,-1,-1,-1]});
  await prepare(sick.actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});
  for(let n=0;n<25;n++){
    await sick.act({keys:[],duration_ms:200,target:{entity:grave.id,camera_angle:-Math.PI/4,fire:true}});
    if((await sick.inspect()).diagnostic.entities.find(e=>e.id===grave.id)?.stage>0)break;
  }
  for(let n=0;n<40&&!((await sick.observe()).character.conditions&bit(5));n++)await sick.act({keys:[],duration_ms:200});
  game.check('natural Kage mine applies Sick',(await sick.observe()).character.conditions&bit(5));
  await prepare(sick.actor,{kind:'teleport',position:realmSpawn});
  await prepare(sick.actor,{kind:'vitals',hp:100,mp:252});
  sample=await use(sick,'1',4,'holy-water-sick-first',true);
  game.check('first Holy Water consumes and cures Sick without its 100 HP heal',Boolean(sample.before.character.conditions&bit(5))&&!(sample.after.character.conditions&bit(5))&&sample.after.character.hp>=100&&sample.after.character.hp<=100+hpRegen(sample));
  sample=await use(sick,'2',5,'holy-water-sick-second',true);
  game.check('second Holy Water heals after Sick was removed',!(sample.before.character.conditions&bit(5))&&sample.after.character.hp-sample.before.character.hp>=100&&sample.after.character.hp-sample.before.character.hp<=100+hpRegen(sample));
}
