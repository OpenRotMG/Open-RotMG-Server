import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const groups=[
  [0x30e,[0xa5c,0xa7e,0xa5d,0xa7f,0xa80,0xa5e]],
  [0x310,[0xa5f,0xadc,0xa60,0xa94,0xa95,0xa96]],
  [0x321,[0xb05,0xb06,0xb07,0xc14,0xc1f,0xc32]],
  [0x323,[0xc14]],
  [0x325,[0xc32]],
];
const spec=([classId,items])=>({class:classId,level:20,seed:876,world:-2,position:'spawn',equipment:[-1,-1,-1,-1],backpack:[...items,...Array(8-items.length).fill(-1)],stats:classId===0x30e?[670,385,75,25,50,75,40,60]:null});
export const fixture=spec(groups[0]);
const expected=new Map([
  [0xa5c,[0,0,0,2,0,0,0,0]],[0xa7e,[0,0,0,3,0,0,0,1]],
  [0xa5d,[0,10,0,4,0,0,0,1]],[0xa7f,[0,15,0,4,0,0,0,2]],
  [0xa80,[0,20,0,6,0,0,0,2]],[0xa5e,[0,25,0,7,0,0,0,3]],
  [0xa5f,[0,30,0,8,0,0,0,3]],[0xadc,[0,35,1,8,0,0,0,4]],
  [0xa60,[0,40,2,9,0,0,0,5]],[0xa94,[0,45,2,10,0,0,0,5]],
  [0xa95,[0,50,3,11,0,0,0,6]],[0xa96,[0,50,3,12,0,0,0,6]],
  [0xb05,[0,55,4,13,0,0,0,6]],[0xb06,[0,60,4,14,0,0,0,7]],
  [0xb07,[0,65,4,15,0,0,0,7]],[0xc14,[0,45,0,0,0,0,0,8]],
  [0xc1f,[0,50,0,10,0,0,4,4]],[0xc32,[0,55,4,13,0,0,2,5]],
]);
export async function run(game){
  const observations=[];
  for(const [n,group] of groups.entries()){
    if(n)game=await game.join(spec(group));
    await game.act({keys:[],duration_ms:600});
    const initial=await game.inspect(),base=initial.view.character.base_stats;
    for(const [i,item] of group[1].entries()){
      const source={x:634+44*(i%4),y:428+44*Math.floor(i/4)},armor={x:722,y:385};
      await game.act({keys:[],duration_ms:350,drag:{from:source,to:armor}});
      const equipped=await game.inspect(),label=group[0].toString(16)+'/'+item.toString(16);
      // Selected definitions include the 123.5.1 buff, not modern MLab balance: https://wikiwiki.jp/rmd/test/build123.5.1
      game.check(label+' native equip applies every selected bonus',equipped.view.character.items[2]===item&&equipped.view.character.items[i+4]===-1&&equipped.view.character.stats.every((v,j)=>v===base[j]+expected.get(item)[j]));
      game.check(label+' equip commits once without changing base stats',equipped.view.character.inventory_revision===initial.view.character.inventory_revision+2*i+1&&JSON.stringify(equipped.view.character.base_stats)===JSON.stringify(base));
      await game.act({keys:[],duration_ms:350},[0xc14,0xc1f,0xc32,0xb07].includes(item)?{name:'robe-'+label.replace('/','-'),when:()=>true}:undefined);
      await game.act({keys:[],duration_ms:350,drag:{from:armor,to:source}});
      const removed=await game.inspect();
      game.check(label+' native removal restores exact inventory and all stats',JSON.stringify(removed.view.character.items)===JSON.stringify(initial.view.character.items)&&JSON.stringify(removed.view.character.stats)===JSON.stringify(initial.view.character.stats)&&removed.view.character.inventory_revision===equipped.view.character.inventory_revision+1);
      game.check(label+' same living character and bounded vitals',removed.view.lifecycle.alive&&removed.view.session.character===initial.view.session.character&&removed.view.character.hp<=removed.view.character.stats[0]&&removed.view.character.mp<=removed.view.character.stats[1]);
      observations.push({label,initial,equipped,removed});
      await writeFile(join(process.env.QA_RESULTS_DIR,'robes.json'),JSON.stringify(observations));
    }
  }
  game=await game.join({class:0x31e,level:20,seed:876,world:-2,position:'spawn',equipment:[-1,-1,-1,-1],backpack:[0xc32,0xa00,-1,-1,-1,-1,-1,-1],stats:null});
  await game.act({keys:[],duration_ms:600});
  const knight=await game.observe();
  await game.act({keys:[],duration_ms:350,drag:{from:{x:678,y:428},to:{x:634,y:385}}});
  const valid=await game.observe();
  game.check('Knight positive control equips sword once',valid.character.items[0]===0xa00&&valid.character.inventory_revision===knight.character.inventory_revision+1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:428},to:{x:722,y:385}}});
  const denied=await game.observe();
  game.check('Knight refuses robe atomically',JSON.stringify(denied.character.items)===JSON.stringify(valid.character.items)&&JSON.stringify(denied.character.stats)===JSON.stringify(valid.character.stats)&&denied.character.inventory_revision===valid.character.inventory_revision);
}
