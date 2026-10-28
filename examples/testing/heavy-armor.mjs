import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
// Embedded upper tiers are not acquisition requirements: https://web.archive.org/web/20120524234154id_/http://forums.wildshadow.com/wiki/realm-mad-god/armor
const groups=[
  [0x31d,25,[[0xa7b,5],[0xa0f,6],[0xa7c,7],[0xa7d,8],[0xa10,10]]],
  [0x31e,40,[[0xa11,12],[0xadf,13],[0xa12,15],[0xa13,20],[0xa91,21]]],
  [0x31f,25,[[0xa92,22],[0xa93,23],[0xafc,24],[0xafd,25],[0xafe,26]]],
];
const spec=([classId,defense,items])=>({class:classId,level:20,seed:875,world:-2,position:'spawn',equipment:[-1,-1,-1,-1],backpack:[...items.map(row=>row[0]),0xa0d,0xb05,-1],stats:[700,200,50,defense,30,50,40,30]});
const pack=i=>({x:634+44*(i%4),y:428+44*Math.floor(i/4)}),armor={x:722,y:385};
export const fixture=spec(groups[0]);
export async function run(game){
  const observations=[];
  for(const [index,group] of groups.entries()){
    if(index)game=await game.join(spec(group));
    await game.act({keys:[],duration_ms:600});
    const initial=await game.observe();
    const drag=async(from,to,capture)=>game.act({keys:[],duration_ms:500,drag:{from,to}},capture);
    for(const slot of [5,6]){
      await drag(pack(slot),armor);
      game.check(group[0]+' rejects '+slot+' atomically',JSON.stringify((await game.observe()).character)===JSON.stringify(initial.character));
    }
    await drag(pack(0),{x:634,y:385});
    game.check(group[0]+' rejects heavy armor in weapon slot atomically',JSON.stringify((await game.observe()).character)===JSON.stringify(initial.character));
    for(const [slot,[item,defense]] of group[2].entries()){
      const before=await game.observe(),expected={...initial.character.stats,defense:initial.character.stats.defense+defense};
      await drag(pack(slot),armor,item===0xafc?{name:'acropolis-equipped',when:view=>view.character.items[2]===item}:undefined);
      const equipped=await game.observe();
      game.check(item.toString(16)+' equips once',equipped.character.items[2]===item&&equipped.character.items[slot+4]===-1&&equipped.character.inventory_revision===before.character.inventory_revision+1);
      game.check(item.toString(16)+' adds only exact defense above base cap',JSON.stringify(equipped.character.stats)===JSON.stringify(expected));
      if(slot===0){
        await drag(pack(6),armor);
        game.check(group[0]+' rejects incompatible replacement without losing equipped armor',JSON.stringify((await game.observe()).character)===JSON.stringify(equipped.character));
      }
      await drag(armor,pack(slot));
      const removed=await game.observe();
      game.check(item.toString(16)+' removes without stat or inventory drift',JSON.stringify(removed.character.stats)===JSON.stringify(initial.character.stats)&&JSON.stringify(removed.character.items)===JSON.stringify(initial.character.items)&&removed.character.inventory_revision===equipped.character.inventory_revision+1);
      game.check(item.toString(16)+' retains resources fame and living identity',removed.character.hp===initial.character.hp&&removed.character.mp===initial.character.mp&&removed.character.fame===initial.character.fame&&removed.lifecycle.alive&&removed.session.character===initial.session.character);
      observations.push({item,before,equipped,removed});
      await writeFile(join(root,'heavy-armor.json'),JSON.stringify(observations,null,2));
    }
  }
  game=await game.join({...spec(groups[0]),class:0x30e,stats:[670,200,50,25,30,50,40,30]});
  await game.act({keys:[],duration_ms:600});
  const wizard=await game.observe();
  await game.act({keys:[],duration_ms:500,drag:{from:pack(0),to:armor}});
  game.check('Wizard rejects heavy armor atomically',JSON.stringify((await game.observe()).character)===JSON.stringify(wizard.character));
}
