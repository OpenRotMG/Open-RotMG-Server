import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
const groups=[
  [0x300,[0xa14,0xa15,0xa72,0xa73,0xa74,0xa16,0xa17]],
  [0x320,[0x709,0xa18,0xa19,0xa88,0xa89,0xa8a,0xaff]],
  [0x324,[0xb00,0xb01,0xc0a,0xb21,0xc22,0xc29]],
];
const spec=([classId,items])=>({class:classId,level:20,seed:870,world:-2,position:'spawn',equipment:[items[0],-1,-1,-1],backpack:[...items.slice(1),0xa04,...Array(8-items.length).fill(-1)],stats:[700,200,50,25,30,50,40,30]});
export const fixture=spec(groups[0]);
export async function run(game){
  const observations=[];
  for(const [index,group] of groups.entries()){
    if(index)game=await game.join(spec(group));
    await game.act({keys:[],duration_ms:600});
    const initial=await game.observe(),wand=group[1].length-1;
    await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(wand%4),y:428+44*Math.floor(wand/4)},to:{x:634,y:385}}});
    const denied=await game.observe();
    game.check(group[0].toString(16)+' rejects wand atomically',JSON.stringify(denied.character.items)===JSON.stringify(initial.character.items)&&denied.character.inventory_revision===initial.character.inventory_revision);
    for(const [i,item] of group[1].entries()){
      if(i){
        const slot=i-1,before=await game.observe();
        await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:428+44*Math.floor(slot/4)},to:{x:634,y:385}}});
        const equipped=await game.observe();
        game.check(item.toString(16)+' swaps exactly once',equipped.character.items[0]===item&&equipped.character.items[4+slot]===before.character.items[0]&&equipped.character.inventory_revision===before.character.inventory_revision+1);
      }
      await game.act({keys:[],duration_ms:500});
      const before=await game.inspect();
      const action=await game.act({keys:[],mouse:{x:480,y:325},duration_ms:2200},[0xa14,0xa17,0xc0a,0xb21,0xc22,0xc29].includes(item)?{name:'dagger-'+item.toString(16),when:view=>view.verification.shots>before.diagnostic.character.shots}:undefined);
      const after=await game.inspect(),observed=await game.observe(),shots=after.diagnostic.character.shots-before.diagnostic.character.shots;
      const duration=action.steps[0].input_release_wall_ms-action.steps[0].input_started_wall_ms;
      // Selected cadence predates modern dagger buffs: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/Player.as#L887-L908
      const rate=item===0xa17?1.5:item===0xb21?0.9:1,period=Math.floor(1000/(1.5+6.5*50/75)/rate);
      game.check(item.toString(16)+' fires at selected cadence',shots>0&&Math.abs(shots-duration/period)<=2);
      const stats=[...initial.character.stats];if(item===0xc0a)stats[7]+=4;
      game.check(item.toString(16)+' retains selected stats and Cronus-only WIS bonus',JSON.stringify(observed.character.stats)===JSON.stringify(stats));
      game.check(item.toString(16)+' firing preserves MP inventory and living identity',after.view.character.mp===before.view.character.mp&&JSON.stringify(after.view.character.items)===JSON.stringify(before.view.character.items)&&after.view.lifecycle.alive&&after.view.session.character===initial.session.character);
      observations.push({item,shots,duration,period,before,after});
      await writeFile(join(root,'daggers.json'),JSON.stringify(observations,null,2));
    }
  }
}
