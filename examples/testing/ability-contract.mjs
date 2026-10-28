import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
export const fixture={class:0x307,level:20,seed:879,world:-2,position:'spawn',equipment:[-1,0xc07,-1,-1],backpack:[0xa61,0xa23,-1,0xa2e,-1,-1,-1,-1],stats:[700,385,75,25,50,50,40,0]};
export async function run(game){
  const records=[];
  const record=async label=>{const v=await game.inspect();records.push({label,...v});await writeFile(join(process.env.QA_RESULTS_DIR,'ability-observations.json'),JSON.stringify(records));return v.view;};
  const initial=await record('initial');
  await game.act({keys:[' '],mouse:{x:500,y:325},duration_ms:180});
  const thunder=await record('thunder');
  const shots=records.at(-1).diagnostic.own_projectiles;
  game.check('ordinary cursor aim produces one correctly directed authoritative Thunder projectile',shots.length===1&&Math.abs(Math.sin(shots[0].angle+Math.PI/4))<0.001&&Math.cos(shots[0].angle+Math.PI/4)>0.999);
  game.check('Thunder accepted with 100 MP spend and six-second cooldown',thunder.character.mp<=initial.character.mp-99&&thunder.character.ability_ready_ms-thunder.game_ms>5500);
  await game.act({keys:[],duration_ms:250,drag:{from:{x:634,y:428},to:{x:678,y:384}}});
  const swapped=await record('swapped');
  game.check('shorter-cooldown Magic Quiver equips once without resetting the active deadline',swapped.character.items[1]===0xa61&&swapped.character.items[4]===0xc07&&swapped.character.inventory_revision===initial.character.inventory_revision+1&&Math.abs(swapped.character.ability_ready_ms-thunder.character.ability_ready_ms)<100);
  await game.act({keys:[' '],duration_ms:200});
  const denied=await record('cooldown-refusal');
  game.check('native retry before original cooldown cannot spend or replace the deadline',denied.game_ms<denied.character.ability_ready_ms&&denied.character.mp>=swapped.character.mp&&Math.abs(denied.character.ability_ready_ms-swapped.character.ability_ready_ms)<100);
  await game.act({keys:[],duration_ms:Math.ceil(denied.character.ability_ready_ms-denied.game_ms)+100});
  const ready=await record('ready');
  await game.act({keys:[' '],duration_ms:180},{name:'ability-after-swap',when:view=>view.character.mp<ready.character.mp-40});
  const cast=await record('short-cast');
  game.check('after original deadline new quiver spends 50 MP and starts its own 500ms cooldown',cast.character.mp<=ready.character.mp-49&&cast.character.ability_ready_ms-cast.game_ms>0&&cast.character.ability_ready_ms-cast.game_ms<=500);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:678,y:384},to:{x:722,y:428}}});
  const empty=await record('empty');
  await game.act({keys:[],duration_ms:350,drag:{from:{x:766,y:428},to:{x:678,y:384}}});
  const incompatible=await record('incompatible-ability');
  game.check('native Archer cannot equip a Wizard spell even into an empty ability slot',JSON.stringify(incompatible.character.items)===JSON.stringify(empty.character.items)&&incompatible.character.inventory_revision===empty.character.inventory_revision);
  await game.act({keys:[' '],duration_ms:200});
  const refused=await record('empty-refusal');
  game.check('empty equipped slot refuses without spending or inventory change',empty.character.items[1]===-1&&refused.character.mp>=empty.character.mp&&JSON.stringify(refused.character.items)===JSON.stringify(empty.character.items)&&refused.character.inventory_revision===empty.character.inventory_revision);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:428},to:{x:678,y:384}}});
  let spent=await record('thunder-requipped');
  for(let n=0;n<2;n++){
    if(n)await game.act({keys:[],duration_ms:6100});
    await game.act({keys:[' '],duration_ms:180});
    spent=await record('thunder-spend-'+n);
  }
  game.check('ordinary repeated casts leave less than Thunder mana cost',spent.character.mp<100);
  await game.act({keys:[],duration_ms:6100});
  const insufficient=await record('insufficient');
  await game.act({keys:[' '],duration_ms:200});
  const noMana=await record('mana-refusal');
  game.check('insufficient MP refuses without spending or starting cooldown',insufficient.character.mp<100&&noMana.character.mp>=insufficient.character.mp&&noMana.character.ability_ready_ms<=noMana.game_ms&&noMana.character.inventory_revision===insufficient.character.inventory_revision);
  game.check('same living Archer and exact equipped identity',noMana.lifecycle.alive&&noMana.session.character===initial.session.character&&noMana.character.items[1]===0xc07);
}
