import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
// Embedded upper tiers are not acquisition requirements: https://web.archive.org/web/20120524234154id_/http://forums.wildshadow.com/wiki/realm-mad-god/armor
const groups={
  warrior:[0x31d,25,[[0xa7b,5],[0xa0f,6],[0xa7c,7],[0xa7d,8],[0xa10,10]]],
  knight:[0x31e,40,[[0xa11,12],[0xadf,13],[0xa12,15],[0xa13,20],[0xa91,21]]],
  paladin:[0x31f,25,[[0xa92,22],[0xa93,23],[0xafc,24],[0xafd,25],[0xafe,26]]],
  wizard:[0x30e,25,[[0xa7b,5]]],
};
const name=process.env.ARMOR_GROUP;
if(!Object.hasOwn(groups,name))throw Error('Set ARMOR_GROUP to warrior, knight, paladin or wizard');
const [classId,baseDefense,items]=groups[name];
const pack=i=>({x:634+44*(i%4),y:428+44*Math.floor(i/4)}),armor={x:722,y:385};
const stable=({ability_ready_ms,...character})=>JSON.stringify(character);
export const fixture={class:classId,level:20,seed:875,world:-2,position:'spawn',equipment:[-1,-1,-1,-1],backpack:[...items.map(row=>row[0]),...Array(5-items.length).fill(-1),0xa0d,0xb05,-1],stats:[classId===0x30e?670:700,200,50,baseDefense,30,50,40,30]};
export async function run(game){
  const observations=[];
  await game.act({keys:[],duration_ms:600});
  const initial=await game.observe();
  const drag=async(from,to,capture)=>game.act({keys:[],duration_ms:500,drag:{from,to}},capture);
  if(name==='wizard'){
    await drag(pack(0),armor);
    game.check('Wizard rejects heavy armor atomically',stable((await game.observe()).character)===stable(initial.character));
    return;
  }
  for(const slot of [5,6]){
    await drag(pack(slot),armor);
    game.check(name+' rejects '+slot+' atomically',stable((await game.observe()).character)===stable(initial.character));
  }
  await drag(pack(0),{x:634,y:385});
  game.check(name+' rejects heavy armor in weapon slot atomically',stable((await game.observe()).character)===stable(initial.character));
  for(const [slot,[item,defense]] of items.entries()){
    const before=await game.observe(),expected={...initial.character.stats,defense:initial.character.stats.defense+defense};
    await drag(pack(slot),armor,item===0xafc?{name:'acropolis-equipped',when:view=>view.character.items[2]===item}:undefined);
    const equipped=await game.observe();
    game.check(item.toString(16)+' equips once',equipped.character.items[2]===item&&equipped.character.items[slot+4]===-1&&equipped.character.inventory_revision===before.character.inventory_revision+1);
    game.check(item.toString(16)+' adds only exact defense above base cap',JSON.stringify(equipped.character.stats)===JSON.stringify(expected));
    if(slot===0){
      await drag(pack(6),armor);
      game.check(name+' rejects incompatible replacement without losing equipped armor',stable((await game.observe()).character)===stable(equipped.character));
    }
    await drag(armor,pack(slot));
    const removed=await game.observe();
    game.check(item.toString(16)+' removes without stat or inventory drift',JSON.stringify(removed.character.stats)===JSON.stringify(initial.character.stats)&&JSON.stringify(removed.character.items)===JSON.stringify(initial.character.items)&&removed.character.inventory_revision===equipped.character.inventory_revision+1);
    game.check(item.toString(16)+' retains resources fame and living identity',removed.character.hp===initial.character.hp&&removed.character.mp===initial.character.mp&&removed.character.fame===initial.character.fame&&removed.lifecycle.alive&&removed.session.character===initial.session.character);
    observations.push({item,before,equipped,removed});
    await writeFile(join(root,'heavy-armor.json'),JSON.stringify(observations,null,2));
  }
}
