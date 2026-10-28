import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
// Modern leather stats/fame differ; use the selected period definitions: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/db/items.xml
const groups=[
  [0x300,[720,252,50,25,75,75,40,50],[[0xa78,5,0,0,0],[0xa0d,6,0,0,0],[0xa79,7,0,0,0],[0xa7a,8,0,2,0],[0xad0,9,0,2,0],[0xad1,10,0,2,0]]],
  [0x307,[700,252,75,25,50,50,40,50],[[0xad2,12,0,2,0],[0xa0e,13,0,3,0],[0xad3,14,0,3,0],[0xa8e,15,0,4,0],[0xa8f,16,0,4,0],[0xa90,17,0,4,0]]],
  [0x320,[720,252,60,25,75,75,40,60],[[0xaf9,17,0,5,0],[0xafa,18,0,5,0],[0xafb,19,0,5,0],[0xa3e,11,3,3,0],[0xc18,16,3,4,0],[0xc28,14,3,3,3]]],
];
const spec=([classId,stats,items])=>({class:classId,level:20,seed:874,world:-2,position:'spawn',equipment:[-1,-1,-1,-1],backpack:[...items.map(row=>row[0]),0xb05,-1],stats});
const pack=i=>({x:634+44*(i%4),y:428+44*Math.floor(i/4)}),armor={x:722,y:385};
const stableCharacter=({ability_ready_ms,...state})=>JSON.stringify(state);
export const fixture=spec(groups[0]);
export async function run(game){
  const observations=[];
  for(const [index,group] of groups.entries()){
    if(index)game=await game.join(spec(group));
    await game.act({keys:[],duration_ms:600});
    const initial=await game.observe();
    const drag=async(from,to,capture)=>game.act({keys:[],duration_ms:400,drag:{from,to}},capture);
    await drag(pack(6),armor);
    const denied=await game.observe();
    game.check(group[0]+' rejects robe atomically',stableCharacter(denied.character)===stableCharacter(initial.character));
    await drag(pack(0),{x:634,y:385});
    game.check(group[0]+' rejects leather in weapon slot atomically',stableCharacter((await game.observe()).character)===stableCharacter(initial.character));
    for(const [slot,[item,defense,speed,dexterity,attack]] of group[2].entries()){
      const before=await game.observe(),expected={...initial.character.stats};
      for(const [stat,bonus] of Object.entries({defense,speed,dexterity,attack}))expected[stat]+=bonus;
      await drag(pack(slot),armor,item===0xc28?{name:'spectral-equipped',when:view=>view.character.items[2]===item}:undefined);
      const equipped=await game.observe();
      game.check(item.toString(16)+' equips exactly once',equipped.character.items[2]===item&&equipped.character.items[slot+4]===-1&&equipped.character.inventory_revision===before.character.inventory_revision+1);
      game.check(item.toString(16)+' adds exact stats above base caps',JSON.stringify(equipped.character.stats)===JSON.stringify(expected));
      await drag(armor,pack(slot));
      const removed=await game.observe();
      game.check(item.toString(16)+' removes every bonus without drift',JSON.stringify(removed.character.stats)===JSON.stringify(initial.character.stats)&&JSON.stringify(removed.character.items)===JSON.stringify(initial.character.items)&&removed.character.inventory_revision===equipped.character.inventory_revision+1);
      game.check(item.toString(16)+' preserves resources and living identity',removed.character.hp===initial.character.hp&&removed.character.mp===initial.character.mp&&removed.lifecycle.alive&&removed.session.character===initial.session.character);
      observations.push({item,before,equipped,removed});
      await writeFile(join(root,'leather.json'),JSON.stringify(observations,null,2));
    }
  }
}
