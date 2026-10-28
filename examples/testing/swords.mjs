import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
const groups=[
  [0x31e,[0xa00,0xa6c,0xa6d,0xa01,0xa6e,0xa3b,0xa3c]],
  [0x31d,[0xa02,0xa82,0xa83,0xa84,0xa47,0xb0b,0xaf4]],
  [0x31f,[0xaf5,0xa03,0xc01,0xc05,0xb40]],
];
const spec=([classId,items])=>({class:classId,level:20,seed:869,world:-2,position:'spawn',equipment:[items[0],-1,-1,-1],backpack:[...items.slice(1),0xa04,...Array(8-items.length).fill(-1)],stats:[700,200,50,25,30,50,40,30]});
export const fixture=spec(groups[0]);
export async function run(game){
  const observations=[];
  for(const [index,group] of groups.entries()){
    if(index)game=await game.join(spec(group));
    await game.act({keys:[],duration_ms:600});
    const initial=await game.observe(),wand=group[1].length-1;
    await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(wand%4),y:428+44*Math.floor(wand/4)},to:{x:634,y:385}}});
    const denied=await game.observe();
    game.check('class '+group[0]+' rejects wand without inventory mutation',JSON.stringify(denied.character.items)===JSON.stringify(initial.character.items)&&denied.character.inventory_revision===initial.character.inventory_revision);
    for(const [i,item] of group[1].entries()){
      if(i){
        const slot=i-1,before=await game.observe();
        await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:428+44*Math.floor(slot/4)},to:{x:634,y:385}}});
        const equipped=await game.observe();
        game.check(item.toString(16)+' swaps once through native inventory',equipped.character.items[0]===item&&equipped.character.items[4+slot]===before.character.items[0]&&equipped.character.inventory_revision===before.character.inventory_revision+1);
      }
      await game.act({keys:[],duration_ms:500});
      const before=await game.inspect();
      const action=await game.act({keys:[],mouse:{x:480,y:325},duration_ms:2200},i===0||item===0xc01||item===0xc05?{name:'sword-'+item.toString(16),when:view=>view.verification.shots>before.view.verification.shots}:undefined);
      const after=await game.inspect(),shots=after.diagnostic.character.shots-before.diagnostic.character.shots;
      const duration=action.steps[0].game_ms-action.steps[0].before_ms;
      const pellets=item===0xc01?2:1,rate=item===0xc05?0.6:1;
      // The selected client, not modern balance, defines cadence and the 40-degree Demon spread: https://web.archive.org/web/20120601000000id_/http://forums.wildshadow.com/wiki/realm-mad-god/swords
      const period=Math.floor(1000/(1.5+6.5*50/75)/rate);
      game.check(item.toString(16)+' fires at selected cadence',shots%pellets===0&&Math.abs(shots/pellets-duration/period)<=2);
      game.check(item.toString(16)+' preserves stats mana inventory and identity',JSON.stringify(after.view.character.stats)===JSON.stringify(initial.character.stats)&&after.view.character.mp===before.view.character.mp&&JSON.stringify(after.view.character.items)===JSON.stringify(before.view.character.items)&&after.view.lifecycle.alive&&after.view.session.character===initial.session.character);
      observations.push({item,shots,duration,period,before,after});
      await writeFile(join(root,'swords.json'),JSON.stringify(observations,null,2));
    }
  }
}
